/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
