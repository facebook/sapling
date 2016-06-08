/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "Hash.h"

#include <folly/Format.h>
#include <folly/String.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <openssl/sha.h>
#include <string>

using std::string;
using folly::ByteRange;
using folly::StringPiece;

namespace facebook {
namespace eden {

namespace {
Hash::Storage hexToBytes(StringPiece hex);
Hash::Storage byteRangeToArray(ByteRange bytes);
}

Hash::Hash() : bytes_{{0}} {}

Hash::Hash(Storage bytes) : bytes_{bytes} {}

Hash::Hash(ByteRange bytes) : Hash{byteRangeToArray(bytes)} {}

Hash::Hash(StringPiece hex) : Hash{hexToBytes(hex)} {}

ByteRange Hash::getBytes() const {
  return ByteRange{bytes_.data(), bytes_.size()};
}

std::string Hash::toString() const {
  std::string result;
  folly::hexlify(bytes_, result);
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
  auto requiredSize = Hash::RAW_SIZE * 2;
  if (hex.size() != requiredSize) {
    throw std::invalid_argument(folly::sformat(
        "{} should have size {} but had size {}",
        hex,
        requiredSize,
        hex.size()));
  }

  string bytes;
  bool isSuccess = folly::unhexlify(hex, bytes);
  if (!isSuccess) {
    throw std::invalid_argument(folly::sformat(
        "{} could not be unhexlified: likely due to invalid characters", hex));
  }

  Hash::Storage hashBytes;
  std::copy(bytes.begin(), bytes.end(), hashBytes.data());
  return hashBytes;
}

Hash::Storage byteRangeToArray(ByteRange bytes) {
  if (bytes.size() != Hash::RAW_SIZE) {
    throw std::invalid_argument(folly::sformat(
        "{} should have size {} but had size {}",
        static_cast<folly::Range<const char*>>(bytes).toString(),
        static_cast<size_t>(Hash::RAW_SIZE),
        bytes.size()));
  }

  Hash::Storage arr;
  std::copy(bytes.begin(), bytes.end(), arr.data());
  return arr;
}
}

Hash Hash::sha1(const folly::IOBuf* buf) {
  SHA_CTX shaCtx;
  SHA1_Init(&shaCtx);

  folly::io::Cursor c(buf);
  while (true) {
    ByteRange peeked = c.peekBytes();
    if (peeked.empty()) {
      break;
    }
    SHA1_Update(&shaCtx, peeked.data(), peeked.size());
    c.skip(peeked.size());
  }

  Storage hashBytes;
  SHA1_Final(hashBytes.data(), &shaCtx);
  return Hash(hashBytes);
}

Hash Hash::sha1(ByteRange data) {
  Storage hashBytes;
  SHA1(data.data(), data.size(), hashBytes.data());
  return Hash(hashBytes);
}
}
}
