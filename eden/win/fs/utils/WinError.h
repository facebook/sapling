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

#include <string>
#include <system_error>
#include "folly/portability/Windows.h"

namespace facebook {
namespace eden {

class Win32ErrorCategory : public std::error_category {
 public:
  const char* name() const noexcept override;
  std::string message(int error) const override;
  static const std::error_category& get() noexcept;
};

class HResultErrorCategory : public std::error_category {
 public:
  const char* name() const noexcept override;
  std::string message(int error) const override;
  static const std::error_category& get() noexcept;
};

std::string win32ErrorToString(uint32_t error);

} // namespace eden
} // namespace facebook
