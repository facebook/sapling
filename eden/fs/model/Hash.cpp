/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/model/Hash.h"

#include <string>

#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/ssl/OpenSSLHash.h>

#include "eden/fs/digest/Blake3.h"
#include "eden/fs/utils/Throw.h"

using folly::ByteRange;
using folly::range;
using folly::ssl::OpenSSLHash;

namespace facebook::eden {

namespace detail {

void throwInvalidArgument(const char* message, size_t number) {
  throwf<std::invalid_argument>("{}{}", message, number);
}

void throwInvalidArgument(const char* message, std::string_view extra) {
  throwf<std::invalid_argument>("{}{}", message, extra);
}

} // namespace detail

const Hash20 kZeroHash;
const Hash32 kZeroHash32;

const Hash20 kEmptySha1{Hash20::Storage{
    0xda, 0x39, 0xa3, 0xee, 0x5e, 0x6b, 0x4b, 0x0d, 0x32, 0x55,
    0xbf, 0xef, 0x95, 0x60, 0x18, 0x90, 0xaf, 0xd8, 0x07, 0x09}};

const Hash32 kEmptyBlake3{
    "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"};

Hash20 Hash20::sha1(const folly::IOBuf& buf) {
  Storage hashBytes;
  OpenSSLHash::sha1(range(hashBytes), buf);
  return Hash20{hashBytes};
}

Hash20 Hash20::sha1(ByteRange data) {
  Storage hashBytes;
  OpenSSLHash::sha1(range(hashBytes), data);
  return Hash20{std::move(hashBytes)};
}

Hash20 Hash20::sha1(const std::string& str) {
  return sha1(folly::ByteRange{folly::StringPiece{str}});
}

Hash32 Hash32::keyedBlake3(
    const folly::ByteRange key,
    const folly::IOBuf& buf) {
  Blake3 hasher(key);
  for (const auto r : buf) {
    hasher.update(r.data(), r.size());
  }

  Storage hashBytes;
  hasher.finalize(range(hashBytes));
  return Hash32{std::move(hashBytes)};
}

Hash32 Hash32::keyedBlake3(const folly::ByteRange key, ByteRange data) {
  Blake3 hasher(key);
  hasher.update(data);

  Storage hashBytes;
  hasher.finalize(range(hashBytes));
  return Hash32{std::move(hashBytes)};
}

Hash32 Hash32::keyedBlake3(const folly::ByteRange key, const std::string& str) {
  return keyedBlake3(key, folly::ByteRange{folly::StringPiece{str}});
}

Hash32 Hash32::blake3(const folly::IOBuf& buf) {
  Blake3 hasher;
  for (const auto r : buf) {
    hasher.update(r.data(), r.size());
  }

  Storage hashBytes;
  hasher.finalize(range(hashBytes));
  return Hash32{std::move(hashBytes)};
}

Hash32 Hash32::blake3(ByteRange data) {
  Blake3 hasher;
  hasher.update(data);

  Storage hashBytes;
  hasher.finalize(range(hashBytes));
  return Hash32{std::move(hashBytes)};
}

Hash32 Hash32::blake3(const std::string& str) {
  return blake3(folly::ByteRange{folly::StringPiece{str}});
}

std::ostream& operator<<(std::ostream& os, const Hash20& hash) {
  os << hash.toString();
  return os;
}

std::ostream& operator<<(std::ostream& os, const Hash32& hash) {
  os << hash.toString();
  return os;
}

} // namespace facebook::eden
