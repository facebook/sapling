---
name: structured-error-logging
description: >
  Add or review structured error logging in EdenFS daemon (C++) — the
  ErrorLogger / EdenErrorInfo system that feeds the edenfs_errors table. Use when
  writing a catch block or handling a boxed/async exception (folly::Try /
  exception_wrapper / .thenError), instrumenting a failure path, or debugging a
  wrong stack trace or wrong component. Covers throw-site trace rules, component
  choice, errorType, noise filtering, gating, and testing.
metadata:
  oncalls: source_control
  strict: true
  apply_to_path: 'eden/fs/.*\.(cpp|h)$'
  apply_to_user_prompt: '(?i)(error logging|structured error|errorlogger|edenerrorinfo|edenfs_errors|log this error|fromexceptionwithouttrace)'
---

# EdenFS Structured Error Logging

EdenFS records daemon-side failures as structured rows in the `edenfs_errors`
table (Hive + Scuba) via the `ErrorLogger`. Unlike `XLOG(ERR, ...)` — which only
writes to the local log file — structured logging gives every error queryable
columns (component, error code/name, mount point, inode, stack trace, …) so we
can aggregate failures across the fleet, alert on them, and debug them after the
fact.

Logging *correctly* matters: the two most common mistakes — a **wrong stack
trace** and a mislabeled **component** — make the data actively misleading rather
than just incomplete. The rules below prevent both. (If you'd `XLOG(ERR, ...)` an
unexpected failure, consider a structured error too.)

## Key files

| Component | Location |
|-----------|----------|
| Logger entry point | `eden/fs/telemetry/ErrorLogger.h` (`log()`, `isEnabled()`) |
| Builder + factories | `eden/fs/telemetry/EdenErrorInfo.h`, `EdenErrorInfoBuilder.h` |
| Exception wrapper | `eden/fs/telemetry/ErrorArg.h` |
| Field → column mapping | `eden/fs/telemetry/DaemonError.h` (`populate()`) |
| Component enum | `eden/fs/telemetry/EdenComponent.h` |
| Access from a handler | `serverState_->getErrorLogger()` |

## The basic shape

```cpp
try {
  doSomethingThatMayThrow();
} catch (const std::exception& ex) {
  serverState_->getErrorLogger().log(
      EdenErrorInfo::backingStore(ex)        // 1. pick the component, pass the error
          .withMountPoint(mountPath.asString())  // 2. chain optional context
          .withErrorType("blob_import_failed")); // 3. a stable, queryable subtype
}
```

`log()` consumes the builder — you never call `create()` yourself.

## Rule 1 (most important): stack-trace provenance

### How the trace is captured

EdenFS hooks the C++ throw path so that **every `throw` records a backtrace into a
thread-local slot**. Two properties of that slot decide correctness:

1. It is **per-thread** — only the thread that threw has the backtrace.
2. It holds **only the most recent throw on that thread**, and reading it (when
   `log()` runs) **consumes** it.

`ErrorArg(const std::exception&)` opts into reading that slot. So the trace
attached to your error matches your exception **only if the most recent throw on
this thread was the one you're logging, and nothing has thrown since.**

### Where the trace is valid: an inline catch on the throwing thread

The one valid case — pass `ex` directly:

```cpp
try {
  importBlob(id);                      // throws here, on this thread
} catch (const std::exception& ex) {
  // nothing else throws between the catch and the log
  logger.log(EdenErrorInfo::backingStore(ex).withErrorType("blob_import"));
}
```

Watch the "nothing thrown since" part: any `throw` between the catch and the
`log()` — even one thrown-and-caught internally by a helper, a `folly::tryTo`, a
map/`fmt` op — overwrites the slot, so do the `log()` before such work.

### Where `ex`'s trace is wrong → use `fromExceptionWithoutTrace`

In all these cases the slot holds a *different* throw (or none), so passing `ex`
attaches a **confidently wrong** trace (worse than none) — use
`ErrorArg::fromExceptionWithoutTrace(ex)`:

- **Boxed** — `folly::Try` / `exception_wrapper`, or `.thenError` / `.thenTry` /
  `with_exception`.
- **Cross-thread** — the continuation runs on a different thread than the throw.
- **Rethrown/reconstructed** — `newEdenError(ex)`, `std::rethrow_exception` (a new
  throw overwrites the slot).
- **Deferred** — logged after other work that may have thrown.

```cpp
// Visiting a boxed exception (here a folly::exception_wrapper) away from its
// throw site — strip the mismatched trace:
ew.with_exception([&](const std::exception& e) {
  logger.log(EdenErrorInfo::objectStore(ErrorArg::fromExceptionWithoutTrace(e))
                 .withErrorType("checkout_update_error"));
});
```

## Rule 2: pick the component that matches where the failure happened

The component column is how errors are bucketed. `EdenErrorInfo` has one factory
per component — see `EdenErrorInfo.h` for the factories/signatures and
`EdenComponent.h` for the component list.

The non-obvious part is *which* to pick: use the factory for the subsystem where
the failure **actually originated**, which is not always the subsystem whose code
you're editing. E.g. an inode *load* failure surfaces in `InodeMap`, but the data
came from the object store, so it's `objectStore(...)`, not `overlay(...)`. Ask
"where did the thing that failed live?", not "what file am I editing?".

## Rule 3: errorType is a stable, queryable subtype

`withErrorType("...")` distinguishes failure modes within a component so you can
filter/alert on them. Use descriptive, stable lower_snake_case strings (e.g.
`blob_import_failed`). Treat them like enum values — don't reword an existing one
casually; dashboards and alerts key off them.

## Builder context methods

Chain whatever `.with*()` context is useful (mount point, inode, file path, …);
all are optional and return the builder — see `EdenErrorInfoBuilder.h`. One thing
not visible from the signatures: `.withErrorCode()` / `.withErrorName()` are
auto-filled from `std::system_error`, so you don't set them yourself.

## Rule 4: log genuine faults, not expected errors

Skip errors that are normal control flow (e.g. a FUSE/NFS `ENOENT` on a missing
path) — logging them buries the real failures. Where an errno is available,
filter on it.

## Gating

Behavior is controlled by config flags (see `EdenConfig.h`); `log()` no-ops when
disabled, so just call it from the catch:

- `telemetry:enable-error-logging` — master on/off.
- `telemetry:error-scribe-category` — Scribe category for the legacy path.
- `telemetry:enable-stack-trace-upload` — upload captured stack traces to Manifold.
- `telemetry:enable-xplatlogger-errors` — route errors via XplatLogger to the
  Logger (Hive).

## Testing

Components that take an injectable `ErrorLogger&`/`ErrorLogger*` (e.g.
`SaplingBackingStore`, `FuseChannel`, `TestMount`) are unit-testable directly:
inject a capturing logger and assert on what was logged.

- Use `eden/fs/telemetry/test/CapturingScribeLogger.h` to capture emitted events.
- `TestMount` accepts an injectable `ErrorLogger`.
- For an end-to-end smoke test, the `debugLogError` Thrift endpoint throws and
  logs a test error: `eden debug thrift debugLogError`.
