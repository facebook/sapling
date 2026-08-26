---
name: acr-refptr-lambda-capture
metadata:
  oncalls: ['scm_client_infra']
  strict: true
  apply_to_path: 'eden/fs/.*\.(cpp|h)$'
  apply_to_content: 'RefPtr|ObjectFetchContextPtr|EdenStatsPtr|thenValue|thenTry|ensure'
---

# RefPtr Lambda Capture

**Severity: HIGH**

## What to Look For

- `RefPtr`-based types (`ObjectFetchContextPtr`, `EdenStatsPtr`) captured in lambda closures
- Lambda captures in `.thenValue()`, `.thenTry()`, `.thenError()`, `.ensure()` chains
- Nested lambdas that re-capture RefPtr types

## When to Flag

- Workarounds for `RefPtr`'s deleted copy constructor (e.g., extracting a raw pointer, wrapping in `std::shared_ptr`) — these bypass the intended ownership semantics and create lifetime bugs
- A `RefPtr` captured by reference in a lambda passed to `.thenValue()` — the reference will dangle after the enclosing scope returns
- Nested lambdas where the inner lambda captures a `RefPtr` from the outer without calling `.copy()` at each nesting level
- A lambda capturing `[this]` where `this` has a `RefPtr` member accessed in the continuation — if `this` is destroyed before the continuation runs, the `RefPtr` member dangles

## Do NOT Flag

- Captures using `.copy()`: `[ctx = ctx.copy()]` — this is the correct pattern
- Captures using `std::move()`: `[ctx = std::move(ctx)]` — correct when the original is no longer needed
- Raw pointer captures where lifetime is guaranteed by another mechanism (e.g., `EdenMountHandle`)
- Test code using `makeRefPtr` in controlled scopes

## Examples

**BAD (RefPtr captured by implicit copy — won't compile):**
```cpp
return inodeMap_->lookupInode(ino)
    .thenValue([fetchContext](auto&& inode) {
        //          ^^^^^^^^^^^^ RefPtr copy ctor is deleted
        return inode->read(fetchContext);
    });
```

**GOOD (use .copy()):**
```cpp
return inodeMap_->lookupInode(ino)
    .thenValue([fetchContext = fetchContext.copy()](auto&& inode) {
        return inode->read(fetchContext);
    });
```

**BAD (nested lambda without .copy() at each level):**
```cpp
return lookupInode(ino)
    .thenValue([ctx = ctx.copy()](auto&& inode) {
        return inode->stat()
            .thenValue([ctx](auto&& stat) {
                //       ^^^ tries to copy RefPtr again — deleted ctor
                return formatStat(stat, ctx);
            });
    });
```

**GOOD (nested .copy()):**
```cpp
return lookupInode(ino)
    .thenValue([ctx = ctx.copy()](auto&& inode) {
        return inode->stat()
            .thenValue([ctx = ctx.copy()](auto&& stat) {
                return formatStat(stat, ctx);
            });
    });
```

## Evidence

- `RefPtr` capture bugs cause either compilation errors (caught at build time) or use-after-free / dangling reference bugs (caught only at runtime, often intermittently).
