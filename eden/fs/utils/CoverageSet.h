/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <cstddef>
#include <set>

namespace facebook {
namespace eden {

/**
 * Tracks contiguous coverage of intervals. Intervals are added dynamically.
 * Then whether a given interval is fully covered can be queried.
 */
class CoverageSet {
 public:
  /**
   * Removes all intervals from the set.
   */
  void clear();

  /**
   * Returns true if no ranges are covered.
   */
  bool empty() const noexcept;

  /**
   * Adds the interval [begin, end) to the set.
   */
  void add(size_t begin, size_t end);

  /**
   * Returns true if the interval [begin, end) is fully covered by the
   * previously-inserted intervals.
   */
  bool covers(size_t begin, size_t end) const noexcept;

  /**
   * Returns the number of intervals currently being tracked. This function is
   * primarily for tests.
   */
  size_t getIntervalCount() const noexcept;

 private:
  struct Interval {
    size_t begin;
    size_t end;

    bool operator<(const Interval& other) const noexcept {
      return begin < other.begin;
    }
  };

  /**
   * The intervals are non-overlapping and non-adjacent. begin < end for all
   * intervals.
   */
  std::set<Interval> set_;
};

} // namespace eden
} // namespace facebook
