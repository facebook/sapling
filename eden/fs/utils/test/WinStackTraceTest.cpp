/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/WinStackTrace.h"

#ifdef _WIN32
#include <cstdint>
#include <cstdio>

#include <folly/CPortability.h>
#include <folly/portability/GTest.h>

namespace {

// Recurse with a sizable frame that the optimizer cannot collapse:
// FOLLY_NOINLINE prevents inlining, the volatile buffer forces a real stack
// allocation in every frame, and using the recursive result after the call
// prevents tail-call optimization. The depth bound keeps this from being
// recursion on all control paths (which MSVC rejects with C4717) while still
// being effectively infinite: the stack overflows long before it is reached.
FOLLY_NOINLINE int overflowTheStack(int depth) {
  volatile char buffer[4096];
  buffer[0] = static_cast<char>(depth);
  buffer[sizeof(buffer) - 1] = buffer[0];
  if (depth < 100000000) {
    return overflowTheStack(depth + 1) + buffer[sizeof(buffer) - 1];
  }
  return buffer[0];
}

bool exitedWithStackOverflow(int exitCode) {
  // gtest reports the raw Windows exit status as an int, so
  // STATUS_STACK_OVERFLOW (0xC00000FD) appears as a negative value; compare
  // the unsigned representation.
  if (static_cast<uint32_t>(exitCode) != 0xC00000FDu) {
    fprintf(
        stderr,
        "unexpected exit code: 0x%X\n",
        static_cast<unsigned int>(exitCode));
    return false;
  }
  return true;
}

} // namespace

TEST(WinStackTraceTest, stackOverflowDefersToWer) {
  // The death-test child installs the real exception filter and drives a
  // real stack overflow. The filter must not symbolize on the exhausted
  // stack: it writes one static message to stderr and returns
  // EXCEPTION_CONTINUE_SEARCH, so the OS default handling (WER) terminates
  // the child with STATUS_STACK_OVERFLOW as the exit status. If the filter
  // regresses to symbolizing (or holding large locals in its own frame), it
  // double-faults before writing anything and this test fails on the missing
  // stderr message.
  EXPECT_EXIT(
      {
        facebook::eden::installWindowsExceptionFilter();
        overflowTheStack(0);
      },
      exitedWithStackOverflow,
      "stack overflow detected, deferring to WER for crash dump");
}
#endif
