---
name: ACR-fuse-handler-safety
metadata:
  oncalls: ['scm_client_infra']
  strict: true
  apply_to_path: 'eden/fs/(fuse|nfs|inodes)/.*\.(cpp|h)$'
  apply_to_content: 'ENOSYS|FuseDispatcher|NfsDispatcher|fuse_entry_out|EOPNOTSUPP'
---

# FUSE/NFS Handler Safety

**Severity: HIGH**

## What to Look For

- New or modified FUSE/NFS dispatch handlers
- errno values returned to the kernel
- Blocking operations inside FUSE worker threads
- ENOSYS usage

## When to Flag

- Returning `ENOSYS` for an operation that might be supported in the future — `ENOSYS` tells the kernel to permanently stop sending that opcode for the lifetime of the mount
- Returning `ENOSYS` for an operation that is sometimes supported (e.g., conditionally based on config) — use `EOPNOTSUPP` instead
- Synchronous blocking calls (disk I/O, network, sleep) in FUSE dispatch handlers without going through `ImmediateFuture` — this blocks the FUSE worker thread pool
- Returning `EINVAL` from `lookup` when the entry doesn't exist — should be `ENOENT`
- Returning `ENOENT` from `getattr` on a valid inode — should be `ESTALE` if the inode was unlinked
- NFS mutating operations that return one of pre-op/post-op `struct stat` but not the other — returning mismatched pairs breaks NFS client cache coherence (returning `std::nullopt` for both is safe)
- Missing `FUSELL_NOT_IMPL()` fallback in base `FuseDispatcher` for new virtual methods

## Do NOT Flag

- `ENOSYS` for operations EdenFS permanently does not support (`flush`, `fsyncdir`, `link`) — these are correct and documented
- `ENOSYS` for `open`/`opendir` when `FUSE_NO_OPEN_SUPPORT`/`FUSE_NO_OPENDIR_SUPPORT` flags are set
- `EPERM` for `setattr` of suid/sgid/sticky bits — mounts use `nosuid`, this is intentional
- `EPERM` for hard links (`link()`) — source control doesn't track hard links
- Returning `std::nullopt` for NFS pre/post stats when atomicity can't be guaranteed — this is safer than returning inconsistent stats
- Test dispatchers (`TestDispatcher`, `FakeFuse`)

## Examples

**BAD (ENOSYS for conditionally-supported operation):**
```cpp
ImmediateFuture<fuse_entry_out> MyDispatcher::lookup(...) {
  if (!featureEnabled()) {
    // WRONG: ENOSYS tells kernel to never send lookup again for this mount
    return folly::makeSemiFuture<fuse_entry_out>(
        std::system_error(ENOSYS, std::generic_category()));
  }
  // ...
}
```

**GOOD (EOPNOTSUPP for conditional support):**
```cpp
if (!featureEnabled()) {
  return folly::makeSemiFuture<fuse_entry_out>(
      std::system_error(EOPNOTSUPP, std::generic_category()));
}
```

**BAD (blocking in FUSE handler):**
```cpp
ImmediateFuture<struct stat> MyDispatcher::getattr(InodeNumber ino, ...) {
  // WRONG: synchronous file read blocks FUSE worker thread
  auto data = folly::readFile(overlayPath.c_str());
  return makeStat(data);
}
```

**GOOD (async through ImmediateFuture):**
```cpp
ImmediateFuture<struct stat> MyDispatcher::getattr(InodeNumber ino, ...) {
  return inodeMap_->lookupInode(ino)
      .thenValue([](auto&& inode) {
        return inode->stat(objectFetchContext);
      });
}
```

**BAD (NFS returning only post-op stat — inconsistent pair):**
```cpp
NfsDispatcher::NfsMutationResult MyNfsDispatcher::setattr(...) {
  auto postStat = inode->setattr(attr);
  // WRONG: returning post-op stat but not pre-op stat
  // NFS client can't compute cache invalidation correctly
  return NfsMutationResult{std::nullopt, postStat};
}
```

**GOOD (return both or neither):**
```cpp
NfsDispatcher::NfsMutationResult MyNfsDispatcher::setattr(...) {
  auto preStat = inode->stat();
  auto postStat = inode->setattr(attr);
  return NfsMutationResult{preStat, postStat};
  // OR if atomicity can't be guaranteed:
  // return NfsMutationResult{std::nullopt, std::nullopt};
}
```

## Recommendation

Use `EOPNOTSUPP` or `ENOTSUP` for conditionally-unavailable operations. Never block FUSE worker threads — all I/O must go through `ImmediateFuture` chains. For NFS, always attempt to return both pre-op and post-op `struct stat` for mutating operations; return `std::nullopt` for both (not just one) when atomicity can't be guaranteed.
