/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/fuse/EdenStats.h"

#include <folly/container/Array.h>
#include <chrono>

using namespace folly;
using namespace std::chrono;

namespace {
constexpr std::chrono::microseconds kMinValue{0};
constexpr std::chrono::microseconds kMaxValue{10000};
constexpr std::chrono::microseconds kBucketSize{1000};
} // namespace

namespace facebook {
namespace eden {

EdenStats::EdenStats() {}

EdenStats::Histogram EdenStats::createHistogram(const std::string& name) {
  return Histogram{this,
                   name,
                   static_cast<size_t>(kBucketSize.count()),
                   kMinValue.count(),
                   kMaxValue.count(),
                   facebook::stats::COUNT,
                   50,
                   90,
                   99};
}

void EdenStats::recordLatency(
    HistogramPtr item,
    std::chrono::microseconds elapsed,
    std::chrono::seconds now) {
  (void)now; // we don't use it in this code path
  (this->*item).addValue(elapsed.count());
}

} // namespace eden
} // namespace facebook
