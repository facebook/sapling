/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <cstdint>

namespace facebook::eden {

enum class ImportPriorityKind : int16_t { Low = 0, Normal = 1, High = 2 };

struct ImportPriority {
  ImportPriorityKind kind;
  uint64_t offset : 48;

  static constexpr ImportPriority kLow() {
    return ImportPriority{ImportPriorityKind::Low};
  }

  static constexpr ImportPriority kNormal() {
    return ImportPriority{ImportPriorityKind::Normal};
  }

  static constexpr ImportPriority kHigh() {
    return ImportPriority{ImportPriorityKind::High};
  }

  // set half of the maximum offset as default offset to allow equal
  // space for raising and lowering priority offset.
  explicit constexpr ImportPriority() noexcept
      : kind(ImportPriorityKind::Normal), offset(0x7FFFFFFFFFFF) {}
  explicit constexpr ImportPriority(ImportPriorityKind kind) noexcept
      : kind(kind), offset(0x7FFFFFFFFFFF) {}
  constexpr ImportPriority(ImportPriorityKind kind, uint64_t offset) noexcept
      : kind(kind), offset(offset) {}

  constexpr inline int64_t value() const noexcept {
    return (static_cast<int16_t>(kind) * (static_cast<uint64_t>(1) << 48)) +
        offset;
  }

  /**
   * Deprioritize ImportPriority by decreasing offset by delta.
   * Note: this function maintains ImportPriorityKind, as jobs
   * with higher priority kind are usually designed to be scheduled
   * ealier and should not lower their kind even when deprioritized.
   */
  constexpr ImportPriority getDeprioritized(uint64_t delta) const noexcept {
    return ImportPriority{kind, offset > delta ? offset - delta : 0};
  }

  friend bool operator<(
      const ImportPriority& lhs,
      const ImportPriority& rhs) noexcept {
    return lhs.value() < rhs.value();
  }

  friend bool operator>(
      const ImportPriority& lhs,
      const ImportPriority& rhs) noexcept {
    return lhs.value() > rhs.value();
  }
};

} // namespace facebook::eden
