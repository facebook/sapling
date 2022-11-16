/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <assert.h>
#include <stdint.h>
#include <string_view>
#include <type_traits>

#include <fmt/ostream.h>

namespace facebook::eden {

/**
 * 64-bit priority value. Effectively a pair of (ImportPriority::Class, offset),
 * where offset is a signed integer used for dynamic priority adjustments.
 *
 * Dynamic priority adjustments do not change the priority class.
 */
class ImportPriority {
 public:
  enum class Class : uint8_t {
    Low = 6,
    Normal = 8,
    High = 10,
  };

  explicit constexpr ImportPriority(
      Class cls = Class::Normal,
      int64_t adjustment = 0) noexcept
      : value_{encode(cls, kDefaultOffset, adjustment)} {}

  static constexpr ImportPriority minimumValue() noexcept {
    return ImportPriority{0};
  }

  /**
   * Returns the priority class component of the priority value.
   */
  constexpr Class getClass() const noexcept {
    return static_cast<Class>(value_ >> kClassShift);
  }

  /**
   * Returns the adjustment component of the priority value.
   */
  constexpr int64_t getAdjustment() const noexcept {
    uint64_t offset = value_ & kOffsetMask;
    return static_cast<int64_t>(offset) - kDefaultOffset;
  }

  /**
   * Returns a human-readable priority class name.
   */
  std::string_view className() const noexcept;

  /**
   * Returns an opaque uint64_t whose only guarantee is that it can be sorted
   * the same way an ImportPriority can.
   */
  constexpr uint64_t value() const noexcept {
    return value_;
  }

  /**
   * Returns a new ImportPriority at a given offset from this. Positive values
   * increase priority; negatives decrease.
   *
   * The priority class will not change. This is intentional, as jobs with high
   * priority class are usually designed to be scheduled earlier even under
   * dynamic prioritization. However, it's somewhat academic, as 60 bits is
   * overkill.
   */
  constexpr ImportPriority adjusted(int64_t delta) const noexcept {
    Class k = getClass();
    uint64_t offset = value_ & kOffsetMask;
    return ImportPriority{encode(k, offset, delta)};
  }

  friend bool operator==(ImportPriority lhs, ImportPriority rhs) noexcept {
    return lhs.value_ == rhs.value_;
  }

  friend bool operator!=(ImportPriority lhs, ImportPriority rhs) noexcept {
    return lhs.value_ != rhs.value_;
  }

  friend bool operator<(ImportPriority lhs, ImportPriority rhs) noexcept {
    return lhs.value_ < rhs.value_;
  }

  friend bool operator<=(ImportPriority lhs, ImportPriority rhs) noexcept {
    return lhs.value_ <= rhs.value_;
  }

  friend bool operator>(ImportPriority lhs, ImportPriority rhs) noexcept {
    return lhs.value_ > rhs.value_;
  }

  friend bool operator>=(ImportPriority lhs, ImportPriority rhs) noexcept {
    return lhs.value_ >= rhs.value_;
  }

 private:
  explicit constexpr ImportPriority(uint64_t raw) noexcept : value_{raw} {}

  static constexpr uint64_t
  encode(Class cls, uint64_t initialOffset, int64_t adjustment) noexcept {
    uint64_t k = static_cast<std::underlying_type_t<Class>>(cls);
    assert(k < 16 && "Priority class must fit in a nibble");

    assert(
        0 == (initialOffset >> kClassShift) &&
        "Initial offset must not overflow into class bits");

    // There has to be a better way of writing the following clamp operation.
    // Another approach would be to ignore clamping entirely, because 60 bits is
    // plenty.
    uint64_t offset = initialOffset;
    if (adjustment > 0) {
      uint64_t positiveOffset = static_cast<uint64_t>(adjustment);
      if (offset > kMaximumOffset - positiveOffset) {
        offset = kMaximumOffset;
      } else {
        offset += positiveOffset;
      }
    } else if (adjustment < 0) {
      uint64_t negativeOffset = static_cast<uint64_t>(-adjustment);
      if (offset < negativeOffset) {
        offset = 0;
      } else {
        offset -= negativeOffset;
      }
    }

    assert(
        0 == (offset >> kClassShift) &&
        "Adjusted offset must not overflow into class bits");
    return (k << kClassShift) | offset;
  }

  // Class is stored in the high nibble. Set the default offset to the midpoint
  // of a 60-bit integer so we can increase and decrease priority without
  // overflowing into the class bits.
  static inline constexpr uint64_t kDefaultOffset = 0x0800'0000'0000'0000ull;
  static inline constexpr uint64_t kMinimumOffset = 0ull;
  static inline constexpr uint64_t kMaximumOffset = 0x0FFF'FFFF'FFFF'FFFFull;
  static inline constexpr uint64_t kClassShift = 60ull;
  static inline constexpr uint64_t kOffsetMask = (1ull << kClassShift) - 1ull;

  uint64_t value_;
};

// Centralized list of default priorities so their relative order is clear.

inline constexpr ImportPriority kDefaultImportPriority{
    ImportPriority::Class::Normal};
inline constexpr ImportPriority kDefaultFsImportPriority{
    ImportPriority::Class::High};
inline constexpr ImportPriority kReaddirPrefetchPriority{
    ImportPriority::Class::Low};
inline constexpr ImportPriority kThriftPrefetchPriority{
    ImportPriority::Class::Low};

} // namespace facebook::eden

namespace std {
inline std::ostream& operator<<(
    std::ostream& os,
    facebook::eden::ImportPriority priority) {
  return os << "(" << priority.className() << ", " << priority.getAdjustment()
            << ")";
}
} // namespace std

template <>
struct fmt::formatter<facebook::eden::ImportPriority> {
  template <typename Context>
  auto format(facebook::eden::ImportPriority priority, Context& ctx) const {
    return format_to(
        ctx.out(),
        "({}, {:+d})",
        priority.className(),
        priority.getAdjustment());
  }
};
