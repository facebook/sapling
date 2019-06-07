/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/utils/Memory.h"
#include <folly/FBString.h>
#include <folly/String.h>
#include <folly/portability/Stdlib.h>

namespace facebook {
namespace eden {
size_t estimateIndirectMemoryUsage(const std::string& path) {
  size_t length = path.length();
#ifdef _LIBCPP_VERSION
  constexpr size_t kStdStringSsoLength = sizeof(std::string) - 2;
#elif defined(__GLIBCXX__) || defined(__GLIBCPP__)
  constexpr size_t kStdStringSsoLength = (sizeof(std::string) >> 1) - 1;
#else
  constexpr size_t kStdStringSsoLength = 0;
  NOT_IMPLEMENTED();
#endif
  if (length <= kStdStringSsoLength) {
    return 0;
  } else {
    return folly::goodMallocSize(path.capacity());
  }
}

size_t estimateIndirectMemoryUsage(const folly::fbstring& path) {
  size_t length = path.length();
  constexpr size_t kFbStringSsoLength = 23;
  if (length <= kFbStringSsoLength) {
    return 0;
  } else {
    return folly::goodMallocSize(path.capacity());
  }
}
} // namespace eden
} // namespace facebook
