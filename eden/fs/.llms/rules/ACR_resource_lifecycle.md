---
name: ACR-resource-lifecycle
metadata:
  oncalls: ['scm_client_infra']
  strict: true
  apply_to_path: 'eden/fs/.*\.(cpp|h|rs|py)$'
  apply_to_content: 'MALLOC_CONF|env_allowlist|envMap|prefetch|Prefetch|F14FastMap|unordered_map|CancellationToken|stop_token|stopWithTimeout'
---

# Resource Lifecycle and Semantic Safety

**Severity: HIGH**

Patterns identified from analysis of ~85 EdenFS SEVs (2023-2026). Each pattern
below has caused at least one SEV.

## When to Flag

### Unbounded Memory Growth (S530151, S601021, S524257, S552094)

- New cache, inode tracker, or prefetch path with no upper bound or eviction
  policy
- Prefetch scope expansion (more files, more platforms) without measuring memory
  impact on constrained environments (16GB VMs, macOS Jetsam threshold)
- Changes that increase loaded inode count without monitoring — inode count grows
  monotonically and degrades all source control operations proportionally

### Unsafe Shutdown / Use-After-Free (S532385, S545432)

- Server shutdown that destroys objects while outstanding Thrift requests still
  hold references — leaked requests completing after `EdenServer` destruction
  cause NPE
- Process lifecycle coupling where EdenFS inherits another process's cgroup —
  when that cgroup is killed, EdenFS dies as collateral

### Environment Variable Inheritance (S513355, S381682, S596057)

- Exposing EdenFS to inherited env vars from parent process (systemd, Chef,
  launchd) — aggressive `MALLOC_CONF` from systemd caused CPU spikes and SSL
  timeouts
- Missing variables in env var allowlist when starting the daemon — `SANDCASTLE`
  omission caused 130-minute build hangs from cache thrashing
- PATH not explicitly set when EdenFS is started from Chef/launchd

### Optimization Altering Semantics (S566000, S538113)

- Changing container type from ordered to unordered (e.g., `std::map` →
  `F14FastMap`) when the container's iteration order is exposed to FUSE/NFS
  responses or determines which entry wins in a dedup/merge
- On case-insensitive filesystems (macOS, Windows), changing dedup "first wins"
  behavior causes phantom files or checkout conflicts

## Do NOT Flag

- Memory changes with explicit bounds, eviction, and alerting already in place
- Shutdown code that uses request cancellation via `CancellationToken` or
  `folly::stop_token` before destroying server objects
- Test code using `TestMount` or `FakeBackingStore` (controlled environments)
- Environment variable changes in test harnesses

## Examples

**BAD (shutdown use-after-free — S532385):**
```cpp
EdenServer::~EdenServer() {
  // Outstanding Thrift requests may still reference this object
  thriftServer_->stop();
  // Leaked requests complete AFTER this destructor runs → NPE
}
```

**GOOD (cancel outstanding requests before destruction):**
```cpp
EdenServer::shutdown() {
  cancellationSource_.requestCancellation();
  // Wait for in-flight requests to drain with timeout
  thriftServer_->stopWithTimeout(shutdownTimeout_);
}
```

**BAD (optimization changes case-insensitive dedup — S566000):**
```cpp
// Changed from map insertion (which kept first entry) to vector append
std::vector<DirEntry> entries;
for (auto& [name, entry] : rawEntries) {
  entries.push_back(entry);  // On case-insensitive FS, "Foo" and "foo" both added
}
// WRONG: changed which entry "wins" vs the old map-based dedup
```

**GOOD (preserve dedup semantics across data structures):**
```cpp
folly::F14FastMap<PathComponent, DirEntry, CaseInsensitiveHash> seen;
std::vector<DirEntry> entries;
for (auto& [name, entry] : rawEntries) {
  auto [it, inserted] = seen.emplace(name, entry);
  if (inserted) {
    entries.push_back(entry);  // Same "first wins" as before
  }
}
```

## Evidence

| Pattern | SEVs | Combined Impact |
|---------|------|-----------------|
| Unbounded memory / OOM | S530151, S601021, S552094, S524257 | 400+ users/day for 5 months |
| Shutdown use-after-free | S532385 | ~1300 crashes/week for 4 months |
| Env var inheritance | S513355, S381682, S596057 | SSL timeouts, 130-min build hangs |
| Optimization semantic change | S566000, S538113 | Phantom files requiring reclone |
