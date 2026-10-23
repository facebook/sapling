---
name: acr-thread-pool-batching
metadata:
  oncalls: ['scm_client_infra']
  strict: true
  apply_to_path: 'eden/fs/.*\.(cpp|h|rs|py)$'
  apply_to_content: 'CPUThreadPoolExecutor|ThreadPoolExecutor|IGNORE_RESULT|addBatch|processBatch'
---

# Thread Pool Self-Deadlock and Request Batching Type Confusion

**Severity: HIGH**

Patterns identified from analysis of ~85 EdenFS SEVs (2023-2026). Each pattern
below has caused at least one SEV.

## When to Flag

### Thread Pool Self-Deadlock (S412223, S399431)

- A bounded thread pool where handlers enqueue work back to the SAME pool — if
  the pool is full, all threads block waiting for themselves, causing deadlock
- A single long-running operation (checkout, prefetch) that can consume all
  Thrift worker threads, starving every other operation

### Request Batching Type Confusion (S561997)

- Requests of different types (e.g., prefetch vs normal read) merged into a
  single batch where a batch-level flag (like `IGNORE_RESULT`) corrupts the
  contract for one type — every request in a batch must get results matching
  its own type

## Do NOT Flag

- Existing well-known bounded executors with documented deadlock-free guarantees
- Test code using `TestMount` or `FakeBackingStore` (controlled environments)

## Examples

**BAD (bounded pool self-deadlock — S412223):**
```cpp
// FSChannel uses a bounded CPUThreadPoolExecutor
auto executor = std::make_shared<folly::CPUThreadPoolExecutor>(numCores);
// Handler enqueues sub-work to the same executor
executor->add([executor]() {
  auto result = doPartialWork();
  // DEADLOCK: if all threads are in this handler, this blocks forever
  executor->add([result]() { finalizeWork(result); });
});
```

**GOOD (separate executor for sub-tasks, or use unbounded queue):**
```cpp
// Option 1: dedicated executor for sub-tasks
auto subExecutor = std::make_shared<folly::CPUThreadPoolExecutor>(numCores);
executor->add([subExecutor]() {
  auto result = doPartialWork();
  subExecutor->add([result]() { finalizeWork(result); });
});

// Option 2: use ImmediateFuture chaining (no re-enqueue)
return doPartialWork()
    .thenValue([](auto&& result) { return finalizeWork(result); });
```

**BAD (batch type confusion — S561997):**
```cpp
void processBatch(std::vector<Request>& batch) {
  bool isPrefetch = std::any_of(batch.begin(), batch.end(),
      [](auto& r) { return r.isPrefetch(); });
  if (isPrefetch) {
    // WRONG: non-prefetch callers in this batch get nullptr results
    for (auto& req : batch) {
      req.setIgnoreResult(true);
    }
  }
}
```

**GOOD (per-request semantics preserved):**
```cpp
void processBatch(std::vector<Request>& batch) {
  for (auto& req : batch) {
    if (req.isPrefetch()) {
      req.setIgnoreResult(true);
    }
    // Non-prefetch requests retain their result contract
  }
}
```

## Evidence

| Pattern | SEVs | Combined Impact |
|---------|------|-----------------|
| Thread pool deadlock/starvation | S412223, S399431, S494560 | Months-long mount hangs |
| Request batch type confusion | S561997 | Spurious buck build failures fleet-wide |
