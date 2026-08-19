---
name: acr-lock-ordering
metadata:
  oncalls: ['scm_client_infra']
  strict: true
  apply_to_path: 'eden/fs/.*\.(cpp|h)$'
  apply_to_content: 'contents_|InodeMap|RenameLock|location_|\.lock\(|\.wlock\('
---

# Inode Lock Ordering

**Severity: CRITICAL**

## What to Look For

- New lock acquisitions that violate the documented ordering in `eden/fs/docs/InodeLocks.md`
- Code that holds `InodeMap::data_` while calling methods on `InodeBase` objects
- Lock acquisitions in the wrong order across async continuations (`.thenValue()` chains)

## When to Flag

- Acquiring `TreeInode::contents_` while holding `InodeMap::data_` — `contents_` is higher in the hierarchy
- Acquiring `FileInode` state while holding `InodeMap::data_`
- Holding `InodeMap::data_` while calling inode methods that may acquire `contents_` or `FileInode` state (e.g., `getOrLoadChild`, `stat`, `read`)
- Acquiring a descendant `TreeInode::contents_` before an ancestor's
- Any new lock that may be held concurrently with an existing inode-hierarchy lock, without documenting where it fits in the ordering

## Do NOT Flag

- Lock acquisitions that follow the documented order: `RenameLock` > `TreeInode::contents_` (ancestor before descendant) > `InodeMap::data_` (brief holds) > `FileInode` state > `InodeBase::location_`
- Acquiring `InodeBase::location_` while holding `InodeMap::data_` — `location_` is the leaf lock and this is explicitly permitted by `InodeLocks.md` (used for logging via `getLogPath()`)
- `XLOG` calls while holding any lock — `getLogPath()` acquires `location_` which is the leaf lock
- Test code that uses `TestMount` (controlled single-threaded environment)
- If the ordering in this rule conflicts with `eden/fs/docs/InodeLocks.md`, the docs file is authoritative

## Examples

**BAD (InodeMap lock held while calling methods that acquire higher-level locks):**
```cpp
auto data = inodeMap_->data_.wlock();
auto inode = data->lookupLoadedInode(ino);
// WRONG: getOrLoadChild() acquires TreeInode::contents_ lock,
// which is higher in the hierarchy than InodeMap::data_
auto child = inode.asTreePtr()->getOrLoadChild(name, ctx);
```

**GOOD (release InodeMap lock before acquiring higher-level locks):**
```cpp
InodePtr inode;
{
  auto data = inodeMap_->data_.wlock();
  inode = data->lookupLoadedInode(ino);
}
// InodeMap lock released, safe to call methods that acquire contents_
auto child = inode.asTreePtr()->getOrLoadChild(name, ctx);
```

**BAD (descendant TreeInode locked before ancestor):**
```cpp
auto childContents = childTree->contents_.wlock();
auto parentContents = parentTree->contents_.wlock();  // DEADLOCK risk
```

**GOOD (ancestor before descendant):**
```cpp
auto parentContents = parentTree->contents_.wlock();
auto childContents = childTree->contents_.wlock();
```

**BAD (lock guard surviving across async continuation):**
```cpp
auto data = inodeMap_->data_.wlock();
auto inode = data->lookupLoadedInode(ino);
return inode->doSomethingAsync()
    .thenValue([data = std::move(data)](auto&& result) {
      // WRONG: InodeMap::data_ lock held across async boundary
      // The continuation may run on a different thread while the lock
      // blocks all other InodeMap operations
      return result;
    });
```

**GOOD (release lock before async chain):**
```cpp
InodePtr inode;
{
  auto data = inodeMap_->data_.wlock();
  inode = data->lookupLoadedInode(ino);
}
// Lock released before entering async chain
return inode->doSomethingAsync()
    .thenValue([](auto&& result) {
      return result;
    });
```

## Evidence

- Lock ordering violations in EdenFS cause deadlocks that hang the FUSE/NFS channel, making the entire mount unresponsive until the EdenFS process is killed.
