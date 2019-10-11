/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/benchharness/Bench.h"
#include <inttypes.h>
#include <stdio.h>
#include <time.h>

namespace facebook {
namespace eden {

uint64_t getTime() noexcept {
  timespec ts;
  // CLOCK_MONOTONIC is subject in NTP adjustments. CLOCK_MONOTONIC_RAW would be
  // better but these benchmarks are short and reading CLOCK_MONOTONIC takes 20
  // ns and CLOCK_MONOTONIC_RAW takes 130 ns.
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return ts.tv_sec * 1000000000 + ts.tv_nsec;
}

StatAccumulator measureClockOverhead() noexcept {
  constexpr int N = 10000;

  StatAccumulator accum;

  uint64_t last = getTime();
  for (int i = 0; i < N; ++i) {
    uint64_t next = getTime();
    uint64_t elapsed = next - last;
    accum.add(elapsed);
    last = next;
  }

  return accum;
}

} // namespace eden
} // namespace facebook
