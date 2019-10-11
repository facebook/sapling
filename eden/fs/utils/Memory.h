/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
