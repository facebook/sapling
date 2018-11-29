/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "Hash.h"

#include <folly/Conv.h>
#include <folly/Format.h>
#include <folly/String.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/ssl/OpenSSLHash.h>
#include <string>

using folly::ByteRange;
using folly::range;
using folly::StringPiece;
using folly::ssl::OpenSSLHash;
using std::string;

namespace facebook {
namespace eden {

const Hash kZeroHash;

const Hash kEmptySha1{Hash::Storage{0xda, 0x39, 0xa3, 0xee, 0x5e, 0x6b, 0x4b,
                                    0x0d, 0x32, 0x55, 0xbf, 0xef, 0x95, 0x60,
                                    0x18, 0x90, 0xaf, 0xd8, 0x07, 0x09}};

namespace {
Hash::Storage hexToBytes(StringPiece hex);
Hash::Storage byteRangeToArray(ByteRange bytes);
} // namespace

Hash::Hash(ByteRange bytes) : Hash{byteRangeToArray(bytes)} {}

Hash::Hash(StringPiece hex) : Hash{hexToBytes(hex)} {}

ByteRange Hash::getBytes() const {
  return ByteRange{bytes_.data(), bytes_.size()};
}

folly::MutableByteRange Hash::mutableBytes() {
  return folly::MutableByteRange{bytes_.data(), bytes_.size()};
}

std::string Hash::toString() const {
  std::string result;
  folly::hexlify(bytes_, result);
  return result;
}

size_t Hash::getHashCode() const noexcept {
  static_assert(sizeof(size_t) <= RAW_SIZE, "crazy size_t type");
  size_t result;
  memcpy(&result, bytes_.data(), sizeof(size_t));
  return result;
}

bool Hash::operator==(const Hash& otherHash) const {
  return bytes_ == otherHash.bytes_;
}

bool Hash::operator<(const Hash& otherHash) const {
  return bytes_ < otherHash.bytes_;
}

namespace {
Hash::Storage hexToBytes(StringPiece hex) {
  size_t requiredSize = Hash::RAW_SIZE * 2;
  if (hex.size() != requiredSize) {
    throw std::invalid_argument(folly::sformat(
        "{} should have size {} but had size {}",
        folly::backslashify<std::string>(hex.str()),
        requiredSize,
        hex.size()));
  }

  string bytes;
  bool isSuccess = folly::unhexlify(hex, bytes);
  if (!isSuccess) {
    throw std::invalid_argument(folly::sformat(
        "{} could not be unhexlified: likely due to invalid characters",
        folly::backslashify<std::string>(hex.str())));
  }

  Hash::Storage hashBytes;
  std::copy(bytes.begin(), bytes.end(), hashBytes.data());
  return hashBytes;
}

Hash::Storage byteRangeToArray(ByteRange bytes) {
  if (bytes.size() != Hash::RAW_SIZE) {
    throw std::invalid_argument(folly::sformat(
        "{} should have size {} but had size {}",
        folly::hexlify(bytes),
        static_cast<size_t>(Hash::RAW_SIZE),
        bytes.size()));
  }

  Hash::Storage arr;
  std::copy(bytes.begin(), bytes.end(), arr.data());
  return arr;
}
} // namespace

Hash Hash::sha1(const folly::IOBuf& buf) {
  Storage hashBytes;
  OpenSSLHash::sha1(range(hashBytes), buf);
  return Hash{hashBytes};
}

Hash Hash::sha1(ByteRange data) {
  Storage hashBytes;
  OpenSSLHash::sha1(range(hashBytes), data);
  return Hash{hashBytes};
}

std::ostream& operator<<(std::ostream& os, const Hash& hash) {
  os << hash.toString();
  return os;
}

void toAppend(const Hash& hash, std::string* result) {
  folly::toAppend(hash.toString(), result);
}
} // namespace eden
} // namespace facebook
