/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "Hash.h"

#include <folly/Conv.h>
#include <folly/String.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/ssl/OpenSSLHash.h>
#include <string>

using folly::ByteRange;
using folly::range;
using folly::ssl::OpenSSLHash;

namespace facebook::eden {

const Hash20 kZeroHash;

const Hash20 kEmptySha1{Hash20::Storage{
    0xda, 0x39, 0xa3, 0xee, 0x5e, 0x6b, 0x4b, 0x0d, 0x32, 0x55,
    0xbf, 0xef, 0x95, 0x60, 0x18, 0x90, 0xaf, 0xd8, 0x07, 0x09}};

folly::MutableByteRange Hash20::mutableBytes() {
  return folly::MutableByteRange{bytes_.data(), bytes_.size()};
}

std::string Hash20::toString() const {
  std::string result;
  folly::hexlify(bytes_, result);
  return result;
}

std::string Hash20::toByteString() const {
  return std::string(reinterpret_cast<const char*>(bytes_.data()), RAW_SIZE);
}

size_t Hash20::getHashCode() const noexcept {
  static_assert(sizeof(size_t) <= RAW_SIZE, "crazy size_t type");
  size_t result;
  memcpy(&result, bytes_.data(), sizeof(size_t));
  return result;
}

bool Hash20::operator==(const Hash20& otherHash) const {
  return bytes_ == otherHash.bytes_;
}

bool Hash20::operator<(const Hash20& otherHash) const {
  return bytes_ < otherHash.bytes_;
}

Hash20 Hash20::sha1(const folly::IOBuf& buf) {
  Storage hashBytes;
  OpenSSLHash::sha1(range(hashBytes), buf);
  return Hash20{hashBytes};
}

Hash20 Hash20::sha1(ByteRange data) {
  Storage hashBytes;
  OpenSSLHash::sha1(range(hashBytes), data);
  return Hash20{hashBytes};
}

void Hash20::throwInvalidArgument(const char* message, size_t number) {
  throw std::invalid_argument(folly::to<std::string>(message, number));
}

std::ostream& operator<<(std::ostream& os, const Hash20& hash) {
  os << hash.toString();
  return os;
}

void toAppend(const Hash20& hash, std::string* result) {
  folly::toAppend(hash.toString(), result);
}

} // namespace facebook::eden
