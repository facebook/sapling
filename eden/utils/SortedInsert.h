/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <algorithm>
#include <folly/String.h>

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

struct CompareString {
  inline bool operator()(const folly::fbstring& a, const folly::fbstring& b) {
    return a < b;
  }
};
}
}
