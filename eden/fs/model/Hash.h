/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <array>
#include <cstdint>
#include <cstring>
#include <iosfwd>
#include <string>

#include <fmt/core.h>
#include <folly/CPortability.h>
#include <folly/Range.h>
#include <folly/container/Array.h>

#include <boost/operators.hpp>

namespace folly {
class IOBuf;
}

namespace facebook::eden {

namespace detail {

struct StringTableHexMakeItem {
  constexpr uint8_t operator()(std::size_t index) const noexcept {
    // clang-format off
    return static_cast<uint8_t>(
        index >= '0' && index <= '9' ? index - '0' :
        index >= 'a' && index <= 'f' ? index - 'a' + 10 :
        index >= 'A' && index <= 'F' ? index - 'A' + 10 :
        17);
    // clang-format on
  }
};
inline constexpr std::array<uint8_t, 256> KHexTable =
    folly::make_array_with<256>(StringTableHexMakeItem());

inline constexpr std::array<char, 16> kLookup = {
    // clang-format off
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f'
    // clang-format on
};

template <typename C>
constexpr bool
hexToBytesPtrSafe(const C* data, const size_t length, uint8_t* bytes) {
  static_assert(
      std::is_same_v<C, char> || std::is_same_v<C, uint8_t>,
      "Data could be only char or uint8_t");
  if ((length & 1) == 1) {
    return false;
  }

  for (auto i = 0u, j = 0u; i < length; i += 2) {
    const auto hi = KHexTable[data[i]];
    const auto lo = KHexTable[data[i + 1]];
    if ((hi | lo) & 0x10) {
      // One of the characters wasn't a hex digit
      return false;
    }

    bytes[j++] = (hi << 4) + lo;
  }

  return true;
}

[[noreturn]] void throwInvalidArgument(const char* message, size_t number);

[[noreturn]] void throwInvalidArgument(
    const char* message,
    std::string_view extra);

} // namespace detail

template <size_t RAW_SIZE>
class Hash : public boost::totally_ordered<Hash<RAW_SIZE>> {
 public:
  using Storage = std::array<uint8_t, RAW_SIZE>;

  /**
   * Create a 0-initialized hash
   */
  constexpr Hash() noexcept : bytes_{} {}

  explicit constexpr Hash(const Storage& bytes) : bytes_{bytes} {}

  explicit constexpr Hash(Storage&& bytes) noexcept
      : bytes_{std::move(bytes)} {}

  explicit constexpr Hash(folly::ByteRange bytes)
      : bytes_{constructFromByteRange(bytes)} {}

  /**
   * @param hex is a string of 40 hexadecimal characters.
   */
  explicit constexpr Hash(folly::StringPiece hex)
      : bytes_{constructFromHex(hex)} {}

  constexpr folly::ByteRange getBytes() const {
    return folly::ByteRange{bytes_.data(), bytes_.size()};
  }

  folly::MutableByteRange mutableBytes() {
    return folly::MutableByteRange{bytes_.data(), bytes_.size()};
  }

  /** @return [lowercase] hex representation of this hash. */
  std::string toString() const {
    std::string hexStr(bytes_.size() << 1, '0');
    for (auto i = 0u; i < bytes_.size(); ++i) {
      hexStr[i << 1] = detail::kLookup[(bytes_[i] >> 4) & 0x0F];
      hexStr[(i << 1) + 1] = detail::kLookup[bytes_[i] & 0x0F];
    }

    return hexStr;
  }

  /** @return raw bytes of this hash. */
  std::string toByteString() const {
    return std::string(reinterpret_cast<const char*>(bytes_.data()), RAW_SIZE);
  }

  size_t getHashCode() const noexcept {
    static_assert(sizeof(size_t) <= RAW_SIZE, "crazy size_t type");
    size_t result;
    memcpy(&result, bytes_.data(), sizeof(size_t));
    return result;
  }

  bool operator==(const Hash& otherHash) const {
    return bytes_ == otherHash.bytes_;
  }

  bool operator<(const Hash& otherHash) const {
    return bytes_ < otherHash.bytes_;
  }

 private:
  static constexpr Storage constructFromByteRange(folly::ByteRange bytes) {
    if (bytes.size() != RAW_SIZE) {
      detail::throwInvalidArgument(
          "incorrect data size for Hash constructor from bytes: ",
          bytes.size());
    }

    // Sigh, silence a gcc warning.
    Storage storage{};
    std::memcpy(storage.data(), bytes.data(), RAW_SIZE);
    return storage;
  }

  static constexpr Storage constructFromHex(folly::StringPiece hex) {
    if (hex.size() != (RAW_SIZE * 2)) {
      detail::throwInvalidArgument(
          "incorrect data size for Hash constructor from string: ", hex.size());
    }
    Storage storage{};
    if (!detail::hexToBytesPtrSafe(hex.data(), hex.size(), storage.data())) {
      detail::throwInvalidArgument(
          "invalid hex digit supplied to Hash constructor from string: {}",
          hex);
    }

    return storage;
  }

  Storage bytes_;
};

class Hash20 : public Hash<20> {
 public:
  enum { RAW_SIZE = 20 };

  using Hash<RAW_SIZE>::Hash;

  /**
   * Compute the SHA1 hash of an IOBuf chain.
   */
  static Hash20 sha1(const folly::IOBuf& buf);

  /**
   * Compute the SHA1 hash of a std::string.
   */
  static Hash20 sha1(const std::string& str);

  /**
   * Compute the SHA1 hash of a ByteRange.
   */
  static Hash20 sha1(folly::ByteRange data);
};
using HashRange = folly::Range<const Hash20*>;

class Hash32 : public Hash<32> {
 public:
  enum { RAW_SIZE = 32 };

  using Hash<RAW_SIZE>::Hash;

  /**
   * Compute the Blake3 hash of an IOBuf chain.
   */
  static Hash32 blake3(const folly::IOBuf& buf);

  /**
   * Compute the Blake3 hash of a std::string.
   */
  static Hash32 blake3(const std::string& str);

  /**
   * Compute the Blake3 hash of a ByteRange.
   */
  static Hash32 blake3(folly::ByteRange data);

  /**
   * Compute the keyed Blake3 hash of an IOBuf chain.
   */
  static Hash32 keyedBlake3(
      const folly::ByteRange key,
      const folly::IOBuf& buf);

  /**
   * Compute the keyed Blake3 hash of a std::string.
   */
  static Hash32 keyedBlake3(const folly::ByteRange key, const std::string& str);

  /**
   * Compute the keyed Blake3 hash of a ByteRange.
   */
  static Hash32 keyedBlake3(const folly::ByteRange key, folly::ByteRange data);
};
using Hash32Range = folly::Range<const Hash32*>;

/** A hash object initialized to all zeroes */
extern const Hash20 kZeroHash;

/** A hash object initialized to the SHA-1 of zero bytes */
extern const Hash20 kEmptySha1;

/** A hash object initialized to all zeroes */
extern const Hash32 kZeroHash32;

/** A hash object initialized to the Blake3 of zero bytes */
extern const Hash32 kEmptyBlake3;

} // namespace facebook::eden

namespace std {
template <>
struct hash<facebook::eden::Hash20> {
  size_t operator()(const facebook::eden::Hash20& hash) const noexcept {
    return hash.getHashCode();
  }
};

template <>
struct hash<facebook::eden::Hash32> {
  size_t operator()(const facebook::eden::Hash32& hash) const noexcept {
    return hash.getHashCode();
  }
};
} // namespace std

template <>
struct fmt::formatter<facebook::eden::Hash20> {
  constexpr auto parse(format_parse_context& ctx) {
    return ctx.begin();
  }

  template <typename Context>
  auto format(const facebook::eden::Hash20& h, Context& ctx) const {
    auto out = ctx.out();
    auto bytes = h.getBytes();
    for (uint8_t byte : bytes) {
      *out++ = facebook::eden::detail::kLookup[(byte >> 4) & 0x0F];
      *out++ = facebook::eden::detail::kLookup[byte & 0x0F];
    }
    return out;
  }
};

template <>
struct fmt::formatter<facebook::eden::Hash32> {
  constexpr auto parse(format_parse_context& ctx) {
    return ctx.begin();
  }

  template <typename Context>
  auto format(const facebook::eden::Hash32& h, Context& ctx) const {
    auto out = ctx.out();
    auto bytes = h.getBytes();
    for (uint8_t byte : bytes) {
      *out++ = facebook::eden::detail::kLookup[(byte >> 4) & 0x0F];
      *out++ = facebook::eden::detail::kLookup[byte & 0x0F];
    }
    return out;
  }
};
