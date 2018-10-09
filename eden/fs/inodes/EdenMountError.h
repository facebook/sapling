/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <stdexcept>

namespace facebook {
namespace eden {

class EdenMountError : public std::runtime_error {
 public:
  explicit EdenMountError(const std::string& what) : std::runtime_error{what} {}
  explicit EdenMountError(const char* what) : std::runtime_error{what} {}
};

} // namespace eden
} // namespace facebook
