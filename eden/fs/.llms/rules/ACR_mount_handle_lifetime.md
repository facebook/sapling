---
name: ACR-mount-handle-lifetime
metadata:
  oncalls: ['scm_client_infra']
  strict: true
  apply_to_path: 'eden/fs/.*\.(cpp|h)$'
  apply_to_content: 'EdenMountHandle|mountHandle|lookupMount|\.ensure\(|wrapImmediateFuture'
---

# EdenMountHandle Lifetime

**Severity: HIGH**

## What to Look For

- Thrift handler implementations that use `lookupMount()` to get an `EdenMountHandle`
- Async chains (`.thenValue()`, `.ensure()`) where the `EdenMountHandle` may be dropped before the chain completes
- `EdenMountHandle` not captured in the final `.ensure()` callback

## When to Flag

- An `EdenMountHandle` obtained via `lookupMount()` that is not kept alive through the entire async chain — if it's dropped, the mount can be unmounted while the request is still in progress
- Missing `.ensure([mountHandle, ...] {})` at the end of a Thrift handler's `ImmediateFuture` chain
- `EdenMountHandle` captured only in an early `.thenValue()` but not in a later continuation — if the early continuation completes, the handle is destroyed

## Do NOT Flag

- Synchronous Thrift handlers that complete before returning (no async chain)
- Handlers that use `mountHandle` only to call `getEdenMount()` and then pass the raw `EdenMount&` — as long as `mountHandle` is kept alive through `.ensure()`
- Test code using `TestMount` (mount lifecycle is controlled by the test)
- Handlers following the documented pattern with `mountHandle` in the final `.ensure()`
- Coroutine (`folly::coro::Task` / `co_await`) handlers where `mountHandle` is a local variable in the coroutine body — the coroutine frame keeps locals alive across `co_await` points

## Examples

**BAD (mountHandle dropped before async chain completes):**
```cpp
folly::SemiFuture<Response> EdenServiceHandler::semifuture_myEndpoint(
    std::unique_ptr<Request> params) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2, *params->mountId()->mountPoint());
  auto mountHandle = lookupMount(params->mountId());
  auto& mount = mountHandle.getEdenMount();
  // mountHandle is a local variable — destroyed when this function returns
  // but the async chain below continues executing after return
  return wrapImmediateFuture(
      std::move(helper),
      mount.doSomethingAsync()  // mount may be unmounted mid-operation
  ).semi();
}
```

**GOOD (mountHandle captured in .ensure()):**
```cpp
folly::SemiFuture<Response> EdenServiceHandler::semifuture_myEndpoint(
    std::unique_ptr<Request> params) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2, *params->mountId()->mountPoint());
  auto mountHandle = lookupMount(params->mountId());
  return wrapImmediateFuture(
      std::move(helper),
      waitForPendingWrites(mountHandle.getEdenMount(), *params->sync())
          .thenValue([mountHandle, params = std::move(params)](auto&&) {
            return mountHandle.getEdenMount().doSomethingAsync();
          })
  ).semi();
  // mountHandle is captured in the lambda, so it lives as long as the chain
}
```

**ALSO GOOD (ensure at the end):**
```cpp
return wrapImmediateFuture(
    std::move(helper),
    doWork(mountHandle.getEdenMount())
).ensure([mountHandle, params = std::move(params)] {})
 .semi();
```
