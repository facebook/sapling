/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/InodePressurePolicy.h"

#include <algorithm>
#include <bit>
#include <cmath>

namespace facebook::eden {

InodePressurePolicy::InodePressurePolicy(
    uint64_t minInodeCount,
    uint64_t maxInodeCount,
    std::chrono::seconds fuseTtlMax,
    std::chrono::seconds fuseTtlMin,
    std::chrono::seconds gcCutoffMax,
    std::chrono::seconds gcCutoffMin)
    : minInodeCount_(minInodeCount),
      maxInodeCount_(std::max(maxInodeCount, minInodeCount + 1)),
      fuseTtlMax_(fuseTtlMax),
      gcCutoffMax_(gcCutoffMax),
      logRange_(
          std::log2(
              static_cast<double>(maxInodeCount_) /
              static_cast<double>(std::max(minInodeCount_, uint64_t{1})))),
      fuseTtlLogRatio_(computeLogRatio(fuseTtlMax, fuseTtlMin)),
      gcCutoffLogRatio_(computeLogRatio(gcCutoffMax, gcCutoffMin)) {}

double InodePressurePolicy::computeLogRatio(
    std::chrono::seconds max,
    std::chrono::seconds min) {
  double maxVal = static_cast<double>(max.count());
  double minVal = static_cast<double>(min.count());
  if (maxVal <= 0) {
    maxVal = 1.0;
  }
  if (minVal <= 0) {
    minVal = 1.0;
  }
  if (minVal > maxVal) {
    minVal = maxVal;
  }
  return std::log2(minVal / maxVal);
}

std::chrono::seconds InodePressurePolicy::interpolate(
    uint64_t totalInodeCount,
    std::chrono::seconds max,
    double logRatio) const {
  if (totalInodeCount <= minInodeCount_) {
    return max;
  }

  double maxVal = static_cast<double>(max.count());
  if (maxVal <= 0) {
    maxVal = 1.0;
  }

  if (totalInodeCount >= maxInodeCount_) {
    auto val = static_cast<int64_t>(std::round(maxVal * std::exp2(logRatio)));
    return std::chrono::seconds{std::max(val, int64_t{1})};
  }

  // Fast approximate log2 from IEEE 754 double bit representation.
  // A double stores value = 2^(exponent-1023) * 1.mantissa, so
  // log2(value) ≈ exponent - 1023 + mantissa/2^52. We extract this
  // with a single bit_cast and arithmetic — no transcendental calls.
  double ratio = static_cast<double>(totalInodeCount) /
      static_cast<double>(std::max(minInodeCount_, uint64_t{1}));
  uint64_t bits = std::bit_cast<uint64_t>(ratio);
  // Reinterpret the 64-bit IEEE 754 encoding as a fixed-point log2.
  // The exponent field (bits 62..52) holds biased exponent (bias=1023),
  // and the mantissa field (bits 51..0) is the fractional part of 1.mantissa.
  // Treating the whole thing as an integer and subtracting the bias gives
  // an approximate log2 in 52-bit fixed point.
  constexpr uint64_t kExponentBias = uint64_t{1023} << 52;
  double fracLog2 =
      static_cast<double>(static_cast<int64_t>(bits - kExponentBias)) /
      static_cast<double>(uint64_t{1} << 52);

  // t in [0, 1]: position in log2-space of inode counts
  double t = std::clamp(fracLog2 / logRange_, 0.0, 1.0);

  // Quadratic ease-in: stays near max at low pressure, drops steeply at high
  // value = max * 2^(t^2 * log2(min/max)), equivalent to max * (min/max)^(t^2)
  auto val =
      static_cast<int64_t>(std::round(maxVal * std::exp2(t * t * logRatio)));
  val = std::max(val, int64_t{1});
  return std::chrono::seconds{val};
}

std::chrono::seconds InodePressurePolicy::getFuseTtl(
    uint64_t totalInodeCount) const {
  return interpolate(totalInodeCount, fuseTtlMax_, fuseTtlLogRatio_);
}

std::chrono::seconds InodePressurePolicy::getGcCutoff(
    uint64_t totalInodeCount) const {
  return interpolate(totalInodeCount, gcCutoffMax_, gcCutoffLogRatio_);
}

} // namespace facebook::eden
