/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#pragma once

#include <rust/cxx.h>
#include <memory>
#include <optional>
#include <stdexcept>

namespace sapling {

enum class BackingStoreErrorKind : uint8_t {
  Generic,
  Network,
  IO,
  DataCorruption
};

class SaplingBackingStoreError : public std::runtime_error {
 public:
  explicit SaplingBackingStoreError(const std::string& msg)
      : std::runtime_error(msg),
        kind_(BackingStoreErrorKind::Generic),
        code_(std::nullopt) {}

  SaplingBackingStoreError(
      const std::string& msg,
      BackingStoreErrorKind kind,
      std::optional<int64_t> code)
      : std::runtime_error(msg), kind_(kind), code_(code) {}

  constexpr std::optional<int32_t> code() const {
    return code_;
  }

  constexpr BackingStoreErrorKind kind() const {
    return kind_;
  }

 private:
  const BackingStoreErrorKind kind_;
  const std::optional<int32_t> code_;
};

std::unique_ptr<SaplingBackingStoreError> backingstore_error(
    rust::Str msg,
    BackingStoreErrorKind kind);

std::unique_ptr<SaplingBackingStoreError> backingstore_error_with_code(
    rust::Str msg,
    BackingStoreErrorKind kind,
    int32_t code);

} // namespace sapling
