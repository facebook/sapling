/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include "eden/win/fs/utils/WinError.h"
#include "folly/portability/Windows.h"

namespace facebook {
namespace eden {

class Guid {
 public:
  static GUID generate() {
    GUID id;
    HRESULT result = CoCreateGuid(&id);
    if (FAILED(result)) {
      throw std::system_error(
          result, HResultErrorCategory::get(), "Failed to create a GUID");
    }
    return id;
  }
};

struct CompareGuid {
  bool operator()(const GUID& left, const GUID& right) const noexcept {
    return memcmp(&left, &right, sizeof(right)) < 0;
  }
};

} // namespace eden
} // namespace facebook
