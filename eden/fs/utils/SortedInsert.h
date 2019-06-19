/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <algorithm>

namespace facebook {
namespace eden {

// Generic function to insert an item in sorted order
template <typename T, typename COMP, typename CONT>
inline typename CONT::iterator sorted_insert(CONT& vec, T&& val, COMP compare) {
  auto find =
      std::lower_bound(vec.begin(), vec.end(), std::forward<T>(val), compare);
  if (find != vec.end() && !compare(val, *find)) {
    // Already exists
    return find;
  }
  return vec.emplace(find, val);
}

} // namespace eden
} // namespace facebook
