/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <cstdint>

namespace facebook {
namespace eden {

enum class ImportPriorityKind : int16_t { Low = -1, Normal = 0, High };

struct ImportPriority {
  ImportPriorityKind kind;
  uint64_t offset : 48;

  static constexpr ImportPriority kNormal() {
    return ImportPriority{ImportPriorityKind::Normal};
  }

  static constexpr ImportPriority kHigh() {
    return ImportPriority{ImportPriorityKind::High};
  }

  explicit constexpr ImportPriority()
      : kind(ImportPriorityKind::Normal), offset(0) {}
  explicit constexpr ImportPriority(ImportPriorityKind kind)
      : kind(kind), offset(0) {}
  constexpr ImportPriority(ImportPriorityKind kind, uint64_t offset)
      : kind(kind), offset(offset) {}

  constexpr inline int64_t value() const noexcept {
    return (static_cast<int16_t>(kind) * (static_cast<uint64_t>(1) << 48)) +
        offset;
  }

  friend bool operator<(
      const ImportPriority& lhs,
      const ImportPriority& rhs) noexcept {
    return lhs.value() < rhs.value();
  }
};

} // namespace eden
} // namespace facebook
