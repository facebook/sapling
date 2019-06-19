/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once
#include <combaseapi.h>
#include "eden/fs/win/utils/WinError.h"
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
