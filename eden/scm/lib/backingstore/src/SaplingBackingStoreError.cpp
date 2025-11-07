/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#include "eden/scm/lib/backingstore/include/SaplingBackingStoreError.h"

namespace sapling {

std::unique_ptr<SaplingBackingStoreError> backingstore_error(
    rust::Str msg,
    BackingStoreErrorKind kind) {
  return std::make_unique<SaplingBackingStoreError>(
      SaplingBackingStoreError{std::string(msg), kind, std::nullopt});
}

std::unique_ptr<SaplingBackingStoreError> backingstore_error_with_code(
    rust::Str msg,
    BackingStoreErrorKind kind,
    int32_t code) {
  return std::make_unique<SaplingBackingStoreError>(
      SaplingBackingStoreError{std::string(msg), kind, code});
}
} // namespace sapling
