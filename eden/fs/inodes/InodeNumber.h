/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <assert.h>
#include <fmt/format.h>
#include <folly/logging/xlog.h>
#include <stdint.h>

namespace facebook::eden {

/**
 * Represents ino_t behind a slightly safer API.  In general, it is a bug if
 * Eden produces inode numbers with the value 0, so this class makes it harder
 * to do that on accident.
 */
struct InodeNumber {
  /// Default-initializes the inode number to 0.
  constexpr InodeNumber() noexcept = default;

  /**
   * Initializes with a given nonzero number.  Will assert in debug builds if
   * initialized to zero.
   */
  constexpr explicit InodeNumber(uint64_t ino) noexcept : rawValue_{ino} {
    // This is intentionally an assert() rather than XDCHECK_NE() since
    // XDCHECK_NE is not allowed in constexpr methods.
    assert(0 != rawValue_);
  }

  /**
   * Thrift does not support unsigned numbers, so it's common to instantiate
   * InodeNumber from int64_t.
   */
  static InodeNumber fromThrift(int64_t ino) {
    return InodeNumber{static_cast<uint64_t>(ino)};
  }

  /**
   * Returns a nonzero inode number.  Asserts in debug builds if zero.
   *
   * Use this accessor when handing inode numbers to FUSE.
   */
  uint64_t get() const {
    XDCHECK_NE(0u, rawValue_);
    return rawValue_;
  }

  /**
   * Returns true if initialized with a nonzero inode number.
   */
  constexpr bool hasValue() const {
    return rawValue_ != 0;
  }

  /**
   * Returns true if underlying value is zero.
   */
  constexpr bool empty() const {
    return rawValue_ == 0;
  }

  /**
   * Returns the underlying value whether or not it's zero.  Use this accessor
   * when debugging or in tests.
   */
  constexpr uint64_t getRawValue() const {
    return rawValue_;
  }

 private:
  uint64_t rawValue_{0};
};

inline bool operator==(InodeNumber lhs, InodeNumber rhs) {
  return lhs.getRawValue() == rhs.getRawValue();
}

inline bool operator!=(InodeNumber lhs, InodeNumber rhs) {
  return lhs.getRawValue() != rhs.getRawValue();
}

inline bool operator<(InodeNumber lhs, InodeNumber rhs) {
  return lhs.getRawValue() < rhs.getRawValue();
}
inline bool operator<=(InodeNumber lhs, InodeNumber rhs) {
  return lhs.getRawValue() <= rhs.getRawValue();
}
inline bool operator>(InodeNumber lhs, InodeNumber rhs) {
  return lhs.getRawValue() > rhs.getRawValue();
}
inline bool operator>=(InodeNumber lhs, InodeNumber rhs) {
  return lhs.getRawValue() >= rhs.getRawValue();
}

constexpr InodeNumber operator""_ino(unsigned long long ino) {
  return InodeNumber{ino};
}

/// The inode number of the mount's root directory.
constexpr InodeNumber kRootNodeId = 1_ino;

} // namespace facebook::eden

namespace std {
template <>
struct hash<facebook::eden::InodeNumber> {
  size_t operator()(facebook::eden::InodeNumber n) const {
    // TODO: It may be worth using a different hash function.  The default
    // std::hash for integers is the identity function.  But since we allocate
    // inode numbers monotonically, this should be okay.
    return std::hash<uint64_t>{}(n.getRawValue());
  }
};
} // namespace std

template <>
struct fmt::formatter<facebook::eden::InodeNumber> : formatter<uint64_t> {
  template <typename Context>
  auto format(facebook::eden::InodeNumber n, Context& ctx) const {
    return formatter<uint64_t>::format(n.getRawValue(), ctx);
  }
};
