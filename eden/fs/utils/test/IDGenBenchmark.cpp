/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/utils/IDGen.h"

#include <folly/Benchmark.h>

using namespace facebook::eden;

BENCHMARK(generateUniqueID, iters) {
  for (unsigned i = 0; i < iters; ++i) {
    folly::doNotOptimizeAway(generateUniqueID());
  }
}
