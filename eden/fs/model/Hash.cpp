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
#include <array>
#include <string>

using std::array;
using std::string;
using folly::ByteRange;
using folly::StringPiece;

namespace facebook {
namespace eden {

namespace {
array<uint8_t, Hash::RAW_SIZE> hexToBytes(StringPiece hex);
array<uint8_t, Hash::RAW_SIZE> byteRangeToArray(ByteRange bytes);
}

Hash::Hash(std::array<uint8_t, Hash::RAW_SIZE> bytes) : bytes_(bytes) {}

Hash::Hash(ByteRange bytes) : Hash(byteRangeToArray(bytes)) {}

Hash::Hash(StringPiece hex) : Hash(hexToBytes(hex)) {}

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
array<uint8_t, Hash::RAW_SIZE> hexToBytes(StringPiece hex) {
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

  std::array<uint8_t, Hash::RAW_SIZE> hashBytes;
  std::copy(bytes.begin(), bytes.end(), hashBytes.data());
  return hashBytes;
}

array<uint8_t, Hash::RAW_SIZE> byteRangeToArray(ByteRange bytes) {
  if (bytes.size() != Hash::RAW_SIZE) {
    throw std::invalid_argument(folly::sformat(
        "{} should have size {} but had size {}",
        static_cast<folly::Range<const char*>>(bytes).toString(),
        static_cast<size_t>(Hash::RAW_SIZE),
        bytes.size()));
  }

  array<uint8_t, Hash::RAW_SIZE> arr;
  std::copy(bytes.begin(), bytes.end(), arr.data());
  return arr;
}
}
}
}
