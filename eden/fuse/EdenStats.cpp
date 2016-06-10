/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "EdenStats.h"

#include <folly/Array.h>
#include <chrono>

using namespace folly;
using namespace std::chrono;

namespace {
constexpr std::chrono::microseconds kMinValue{0};
constexpr std::chrono::microseconds kMaxValue{10000};
constexpr std::chrono::microseconds kBucketSize{1000};
constexpr unsigned int kNumTimeseriesBuckets{60};
constexpr auto kDurations = folly::make_array(
    std::chrono::seconds(60),
    std::chrono::seconds(600),
    std::chrono::seconds(3600),
    std::chrono::seconds(0));
}

namespace facebook {
namespace eden {
namespace fusell {

EdenStats::EdenStats() {}

folly::TimeseriesHistogram<int64_t> EdenStats::createHistogram() {
  return folly::TimeseriesHistogram<int64_t>{
      kBucketSize.count(),
      kMinValue.count(),
      kMaxValue.count(),
      MultiLevelTimeSeries<int64_t>{
          kNumTimeseriesBuckets, kDurations.size(), kDurations.data()}};
}
}
}
}
