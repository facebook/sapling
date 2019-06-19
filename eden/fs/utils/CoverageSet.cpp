/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/utils/CoverageSet.h"
#include <folly/logging/xlog.h>

namespace facebook {
namespace eden {

void CoverageSet::clear() {
  set_.clear();
}

bool CoverageSet::empty() const noexcept {
  return set_.empty();
}

void CoverageSet::add(size_t begin, size_t end) {
  using Set = std::set<Interval>;
  using Iter = Set::iterator;

  CHECK_LE(begin, end)
      << "End of interval must be greater than or equal to begin";
  if (begin == end) {
    return;
  }

  Iter right = set_.lower_bound(Interval{begin, end});
  Iter left = right == set_.begin() ? set_.end() : std::prev(right);

  // While the xcode 10 clang compiler is C++17, its libc++ doesn't
  // implement node_type/extract from C++17, so we need to live
  // without it for now.  When that support is available, we can
  // remove this ifdef.
#ifdef __APPLE__
  auto erase = [&](Iter iter) -> void { set_.erase(iter); };
#else
  // To avoid allocation when possible, save up to one node that can be
  // modified before reinsertion.
  Set::node_type reuse_handle;

  auto erase = [&](Iter iter) -> void {
    if (reuse_handle) {
      set_.erase(iter);
    } else {
      reuse_handle = set_.extract(iter);
    }
  };
#endif

  // In the case that the new interval is completely subsumed by an existing
  // interval, this code currently rebalances once on the erase and once on the
  // reinsertion. At the cost of some additional checks, the rebalances could be
  // avoided. This optimization probably isn't worth much under typical usage.

  if (left != set_.end() && left->end == begin) {
    begin = left->begin;
    erase(left);
  }
  while (right != set_.end() && end >= right->begin) {
    auto next = std::next(right);
    end = std::max(end, right->end);
    erase(right);
    right = next;
  }

#ifndef __APPLE__
  if (reuse_handle) {
    reuse_handle.value().begin = begin;
    reuse_handle.value().end = end;
    set_.insert(std::move(reuse_handle));
  } else
#endif
  {
    set_.insert(Interval{begin, end});
  }
}

bool CoverageSet::covers(size_t begin, size_t end) const noexcept {
  CHECK_LE(begin, end)
      << "End of interval must be greater than or equal to begin";
  if (begin == end) {
    return true;
  }

  auto right = set_.upper_bound(Interval{begin, end});
  if (right == set_.begin()) {
    return false;
  }
  auto left = std::prev(right);
  return left->begin <= begin && end <= left->end;
}

size_t CoverageSet::getIntervalCount() const noexcept {
  return set_.size();
}

} // namespace eden
} // namespace facebook
