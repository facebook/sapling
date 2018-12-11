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

#include <boost/operators.hpp>
#include <folly/Range.h>
#include <stdint.h>
#include <array>
#include <iosfwd>

namespace folly {
class IOBuf;
}

namespace facebook {
namespace eden {

/**
 * Immutable 160-bit hash.
 */
class Hash : boost::totally_ordered<Hash> {
 public:
  enum { RAW_SIZE = 20 };
  using Storage = std::array<uint8_t, RAW_SIZE>;

  /**
   * Create a 0-initialized hash
   */
  constexpr Hash() noexcept : bytes_{} {}

  explicit constexpr Hash(const Storage& bytes) : bytes_{bytes} {}

  explicit Hash(folly::ByteRange bytes);

  /**
   * @param hex is a string of 40 hexadecimal characters.
   */
  explicit Hash(folly::StringPiece hex);

  /**
   * Compute the SHA1 hash of an IOBuf chain.
   */
  static Hash sha1(const folly::IOBuf& buf);

  /**
   * Compute the SHA1 hash of a ByteRange
   */
  static Hash sha1(folly::ByteRange data);

  folly::ByteRange getBytes() const;
  folly::MutableByteRange mutableBytes();

  /** @return 40-character [lowercase] hex representation of this hash. */
  std::string toString() const;

  size_t getHashCode() const noexcept;

  bool operator==(const Hash&) const;
  bool operator<(const Hash&) const;

 private:
  Storage bytes_;
};

/** A hash object initialized to all zeroes */
extern const Hash kZeroHash;

/** A hash object initialized to the SHA-1 of zero bytes */
extern const Hash kEmptySha1;

/**
 * Output stream operator for Hash.
 *
 * This makes it possible to easily use Hash in glog statements.
 */
std::ostream& operator<<(std::ostream& os, const Hash& hash);

/* Define toAppend() so folly::to<string>(Hash) will work */
void toAppend(const Hash& hash, std::string* result);
} // namespace eden
} // namespace facebook

namespace std {
template <>
struct hash<facebook::eden::Hash> {
  size_t operator()(const facebook::eden::Hash& hash) const noexcept {
    return hash.getHashCode();
  }
};
} // namespace std
