/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <chrono>
#include <cstdint>

namespace facebook::eden {

/**
 * Computes FUSE TTLs and GC cutoffs based on inode count pressure.
 *
 * Values are continuously interpolated between configurable min/max bounds
 * using a power-curve that ramps slowly at low pressure and aggressively
 * at high pressure:
 *
 *   count <= minInodeCount  → max values (no pressure)
 *   count >= maxInodeCount  → min values (maximum pressure)
 *   between: value = max * (min/max)^(t^2)
 *     where t = log(count/minInode) / log(maxInode/minInode)
 *
 * The t^2 (quadratic ease-in) keeps values near max at low inode counts
 * and drops steeply toward min at high counts.
 */
class InodePressurePolicy {
 public:
  /**
   * Construct from config values.
   *
   * @param minInodeCount Inode count below which no pressure is applied.
   * @param maxInodeCount Inode count at/above which maximum pressure applies.
   * @param fuseTtlMax TTL at the lowest pressure (seconds).
   * @param fuseTtlMin TTL at the highest pressure (seconds).
   * @param gcCutoffMax GC cutoff at the lowest pressure (seconds).
   * @param gcCutoffMin GC cutoff at the highest pressure (seconds).
   */
  InodePressurePolicy(
      uint64_t minInodeCount,
      uint64_t maxInodeCount,
      std::chrono::seconds fuseTtlMax,
      std::chrono::seconds fuseTtlMin,
      std::chrono::seconds gcCutoffMax,
      std::chrono::seconds gcCutoffMin);

  /**
   * Get the FUSE entry/attribute cache TTL for a given inode count.
   */
  std::chrono::seconds getFuseTtl(uint64_t totalInodeCount) const;

  /**
   * Get the GC cutoff duration for a given inode count.
   * Inodes not accessed within this duration are candidates for GC.
   */
  std::chrono::seconds getGcCutoff(uint64_t totalInodeCount) const;

 private:
  std::chrono::seconds interpolate(
      uint64_t totalInodeCount,
      std::chrono::seconds max,
      double logRatio) const;

  static double computeLogRatio(
      std::chrono::seconds max,
      std::chrono::seconds min);

  uint64_t minInodeCount_;
  uint64_t maxInodeCount_;
  std::chrono::seconds fuseTtlMax_;
  std::chrono::seconds gcCutoffMax_;
  double logRange_; // precomputed log2(maxInodeCount/minInodeCount)
  double fuseTtlLogRatio_; // precomputed log2(fuseTtlMin/fuseTtlMax)
  double gcCutoffLogRatio_; // precomputed log2(gcCutoffMin/gcCutoffMax)
};

} // namespace facebook::eden
