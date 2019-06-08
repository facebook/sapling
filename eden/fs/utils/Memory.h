/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <folly/FBString.h>
#include <string>

namespace facebook {
namespace eden {
template <typename StringType>
bool isStringStorageEmbedded(const StringType& t) {
  const void* tbegin = &t;
  const void* tend = &t + 1;
  return std::less_equal<const void*>{}(tbegin, t.data()) &&
      std::less<const void*>{}(t.data(), tend);
}

template <typename StringType>
size_t estimateIndirectMemoryUsage(const StringType& s) {
  if (isStringStorageEmbedded(s)) {
    return 0;
  } else {
    return folly::goodMallocSize(s.capacity());
  }
}
} // namespace eden
} // namespace facebook
