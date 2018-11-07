/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/utils/IDGen.h"

#include <folly/Benchmark.h>

using namespace facebook::eden;

BENCHMARK(generateUniqueID, iters) {
  for (unsigned i = 0; i < iters; ++i) {
    folly::doNotOptimizeAway(generateUniqueID());
  }
}
