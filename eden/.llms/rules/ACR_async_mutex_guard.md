---
oncalls: ['source_control']
apply_to_regex: 'eden/(mononoke|scm)/.*\.rs$'
apply_to_content: '\.lock\(\)|\.read\(\)|\.write\(\)|RwLock|Mutex'
---

# Async Mutex Guard Across Await

**Severity: CRITICAL**

## What to Look For

- `Mutex::lock()`, `RwLock::read()`, or `RwLock::write()` guard held across an `.await` point
- A `let guard = mutex.lock()` followed by any `.await` before `guard` is dropped

## When to Flag

- Any `MutexGuard` or `RwLockReadGuard`/`RwLockWriteGuard` that is live (not dropped) when an `.await` is hit
- Using `std::sync::Mutex` in async code (should use `tokio::sync::Mutex` if the guard must span awaits, or scope it tightly)

## Do NOT Flag

- Guards dropped before the `.await` (e.g., scoped in a block `{ let g = m.lock(); val = g.clone(); }` then `val.do_async().await`)
- `tokio::sync::Mutex` used intentionally with a comment explaining why
- Synchronous code paths (no async fn or .await in scope)

## Examples

**BAD:**
```rust
async fn update_cache(cache: &Mutex<HashMap<Key, Value>>, key: Key) -> Result<()> {
    let mut guard = cache.lock().unwrap();
    let new_val = fetch_from_store(key).await?;  // guard held across await!
    guard.insert(key, new_val);
    Ok(())
}
```

**GOOD:**
```rust
async fn update_cache(cache: &Mutex<HashMap<Key, Value>>, key: Key) -> Result<()> {
    let new_val = fetch_from_store(key).await?;
    // lock() only returns Err on poison (prior panic) — unrecoverable, so expect is fine here
    let mut guard = cache.lock().expect("cache lock poisoned");
    guard.insert(key, new_val);
    Ok(())
}
```

## Recommendation

Restructure code so the lock is acquired *after* all `.await` calls, or acquire-copy-release before awaiting. If you must hold a lock across awaits, use `tokio::sync::Mutex` and add a comment explaining the design choice.
