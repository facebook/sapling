/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/model/ObjectId.h"

#include <folly/Conv.h>
#include <folly/Format.h>
#include <folly/String.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/ssl/OpenSSLHash.h>
#include <string>

using folly::ByteRange;
using folly::range;
using folly::ssl::OpenSSLHash;

namespace facebook::eden {

folly::MutableByteRange ObjectId::mutableBytes() {
  return folly::MutableByteRange{bytes_.data(), bytes_.size()};
}

std::string ObjectId::asHexString() const {
  std::string result;
  folly::hexlify(bytes_, result);
  return result;
}

std::string ObjectId::toByteString() const {
  return std::string(reinterpret_cast<const char*>(bytes_.data()), RAW_SIZE);
}

size_t ObjectId::getHashCode() const noexcept {
  static_assert(sizeof(size_t) <= RAW_SIZE, "crazy size_t type");
  size_t result;
  memcpy(&result, bytes_.data(), sizeof(size_t));
  return result;
}

bool ObjectId::operator==(const ObjectId& otherHash) const {
  return bytes_ == otherHash.bytes_;
}

bool ObjectId::operator<(const ObjectId& otherHash) const {
  return bytes_ < otherHash.bytes_;
}

ObjectId ObjectId::sha1(const folly::IOBuf& buf) {
  Storage hashBytes;
  OpenSSLHash::sha1(range(hashBytes), buf);
  return ObjectId{hashBytes};
}

ObjectId ObjectId::sha1(ByteRange data) {
  Storage hashBytes;
  OpenSSLHash::sha1(range(hashBytes), data);
  return ObjectId{hashBytes};
}

void ObjectId::throwInvalidArgument(const char* message, size_t number) {
  throw std::invalid_argument(folly::to<std::string>(message, number));
}

std::ostream& operator<<(std::ostream& os, const ObjectId& hash) {
  os << hash.toLogString();
  return os;
}

void toAppend(const ObjectId& hash, std::string* result) {
  folly::toAppend(hash.toLogString(), result);
}

} // namespace facebook::eden
