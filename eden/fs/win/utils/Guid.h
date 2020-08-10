/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include "folly/portability/Windows.h"

#include <combaseapi.h>
#include "eden/fs/win/utils/WinError.h"

namespace facebook {
namespace eden {

class Guid {
 public:
  static Guid generate() {
    GUID id;
    HRESULT result = CoCreateGuid(&id);
    if (FAILED(result)) {
      throw std::system_error(
          result, HResultErrorCategory::get(), "Failed to create a GUID");
    }
    return Guid{id};
  }

  Guid() noexcept : guid_{0} {}
  Guid(const GUID& guid) noexcept : guid_{guid} {}

  Guid(const Guid& other) noexcept : guid_{other.guid_} {}

  Guid& operator=(const Guid& other) noexcept {
    guid_ = other.guid_;
    return *this;
  }

  const GUID& getGuid() const noexcept {
    return guid_;
  }

  operator const GUID&() const noexcept {
    return guid_;
  }

  operator const GUID*() const noexcept {
    return &guid_;
  }

  bool operator==(const Guid& other) const noexcept {
    return guid_ == other.guid_;
  }

  bool operator!=(const Guid& other) const noexcept {
    return !(*this == other);
  }

 private:
  GUID guid_;
};

struct CompareGuid {
  bool operator()(const GUID& left, const GUID& right) const noexcept {
    return memcmp(&left, &right, sizeof(right)) < 0;
  }
};

} // namespace eden
} // namespace facebook
