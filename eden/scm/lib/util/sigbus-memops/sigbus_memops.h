/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#ifndef SIGBUS_MEMOPS_H
#define SIGBUS_MEMOPS_H

#ifndef _WIN32
#include <signal.h>
#endif
#include <stdbool.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Return whether SIGBUS or equivalent exception protection is available. */
bool sigbus_is_protected(void);

/*
 * Install a process-wide SIGBUS handler for the protected operations.
 *
 * The handler is installed at most once and delegates unrecognized signals to
 * the handler that was installed previously. Returns 0 on success, otherwise
 * an errno value.
 */
int sigbus_install_handler(void);

/*
 * Set a process-wide budget for retrying unhandled synchronous BUS_ADRERR
 * faults. Each retry consumes one from the budget. The default is zero.
 */
void sigbus_set_retry_budget(unsigned int retry_budget);

#ifndef _WIN32
/*
 * Try to handle a synchronous SIGBUS raised by `sigbus_try_memcpy` or
 * `sigbus_try_read`, or an unhandled BUS_ADRERR when retry budget remains.
 *
 * Intended to be called from an SA_SIGINFO signal handler.
 *
 * A protected load/store redirects the saved execution context (`ucontext`)
 * to its error path. An unhandled BUS_ADRERR leaves the context unchanged, so
 * returning from the handler retries the fault. Other causes, including
 * asynchronous BUS_MCEERR_AO notifications, are not handled.
 *
 * Returns:
 * - true: SIGBUS handled; the signal handler must return immediately.
 * - false: SIGBUS not handled; the signal handler should delegate to a fallback
 *   handler.
 */
bool sigbus_try_handle(int signo, siginfo_t* info, void* ucontext);
#endif

/*
 * Try to copy `len` bytes from `src` to `dst` with SIGBUS protection.
 *
 * On supported targets, returns false if either the source read or destination
 * write raises a recognized synchronous SIGBUS. In that case, `src` might be
 * partially copied to `dst`.
 *
 * On Windows, the equivalent exception is `EXCEPTION_IN_PAGE_ERROR`.
 * `EXCEPTION_ACCESS_VIOLATION` is not handled.
 *
 * On unsupported targets, calls libc memcpy and returns true if memcpy returns
 * normally.
 *
 * `src` and `dst` must not overlap.
 */
bool sigbus_try_memcpy(void* dst, const void* src, size_t len);

/*
 * Read `len` bytes starting at `src` with SIGBUS protection.
 *
 * On supported targets, returns false if a read raises a recognized synchronous
 * SIGBUS.
 *
 * On Windows, the equivalent exception is `EXCEPTION_IN_PAGE_ERROR`.
 * `EXCEPTION_ACCESS_VIOLATION` is not handled.
 *
 * On unsupported targets, the reads are unprotected.
 *
 * For page-granularity fault-in checks, consider calling with `len == 1`
 * per page, for better performance.
 *
 * If `len` is 0, `src` will not be read.
 */
bool sigbus_try_read(const void* src, size_t len);

#ifdef __cplusplus
}
#endif

#endif
