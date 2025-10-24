/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#pragma once

#include <stdexcept>

namespace sapling {

class SaplingBackingStoreError : public std::runtime_error {
 public:
  using std::runtime_error::runtime_error;
};

} // namespace sapling
