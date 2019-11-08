/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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

  explicit constexpr Hash(folly::ByteRange bytes)
      : bytes_{constructFromByteRange(bytes)} {}

  /**
   * @param hex is a string of 40 hexadecimal characters.
   */
  explicit constexpr Hash(folly::StringPiece hex)
      : bytes_{constructFromHex(hex)} {}

  /**
   * Compute the SHA1 hash of an IOBuf chain.
   */
  static Hash sha1(const folly::IOBuf& buf);

  /**
   * Compute the SHA1 hash of a ByteRange
   */
  static Hash sha1(folly::ByteRange data);

  constexpr folly::ByteRange getBytes() const {
    return folly::ByteRange{bytes_.data(), bytes_.size()};
  }
  folly::MutableByteRange mutableBytes();

  /** @return 40-character [lowercase] hex representation of this hash. */
  std::string toString() const;

  size_t getHashCode() const noexcept;

  bool operator==(const Hash&) const;
  bool operator<(const Hash&) const;

 private:
  static constexpr Storage constructFromByteRange(folly::ByteRange bytes) {
    if (bytes.size() != RAW_SIZE) {
      throwInvalidArgument(
          "incorrect data size for Hash constructor from bytes: ",
          bytes.size());
    }
    return {
        bytes.data()[0],  bytes.data()[1],  bytes.data()[2],  bytes.data()[3],
        bytes.data()[4],  bytes.data()[5],  bytes.data()[6],  bytes.data()[7],
        bytes.data()[8],  bytes.data()[9],  bytes.data()[10], bytes.data()[11],
        bytes.data()[12], bytes.data()[13], bytes.data()[14], bytes.data()[15],
        bytes.data()[16], bytes.data()[17], bytes.data()[18], bytes.data()[19]};
  }
  static constexpr Storage constructFromHex(folly::StringPiece hex) {
    if (hex.size() != (RAW_SIZE * 2)) {
      throwInvalidArgument(
          "incorrect data size for Hash constructor from string: ", hex.size());
    }
    return {
        hexByteAt(hex, 0),  hexByteAt(hex, 1),  hexByteAt(hex, 2),
        hexByteAt(hex, 3),  hexByteAt(hex, 4),  hexByteAt(hex, 5),
        hexByteAt(hex, 6),  hexByteAt(hex, 7),  hexByteAt(hex, 8),
        hexByteAt(hex, 9),  hexByteAt(hex, 10), hexByteAt(hex, 11),
        hexByteAt(hex, 12), hexByteAt(hex, 13), hexByteAt(hex, 14),
        hexByteAt(hex, 15), hexByteAt(hex, 16), hexByteAt(hex, 17),
        hexByteAt(hex, 18), hexByteAt(hex, 19),
    };
  }
  static constexpr uint8_t hexByteAt(folly::StringPiece hex, size_t index) {
    return (nibbleToHex(hex.data()[index * 2]) * 16) +
        nibbleToHex(hex.data()[(index * 2) + 1]);
  }
  static constexpr uint8_t nibbleToHex(char c) {
    if ('0' <= c && c <= '9') {
      return c - '0';
    } else if ('a' <= c && c <= 'f') {
      return 10 + c - 'a';
    } else if ('A' <= c && c <= 'F') {
      return 10 + c - 'A';
    } else {
      throwInvalidArgument(
          "invalid hex digit supplied to Hash constructor from string: ", c);
    }
  }

  [[noreturn]] static void throwInvalidArgument(
      const char* message,
      size_t number);

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
