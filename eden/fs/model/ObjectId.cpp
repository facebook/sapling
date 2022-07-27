/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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

#include "eden/fs/model/Hash.h"
#include "eden/fs/utils/Throw.h"

using folly::ByteRange;
using folly::range;
using folly::ssl::OpenSSLHash;

namespace facebook::eden {

std::string ObjectId::asHexString() const {
  auto bytes = getBytes();
  std::string result;
  folly::hexlify(bytes, result);
  return result;
}

std::string ObjectId::asString() const {
  auto bytes = getBytes();
  return std::string(reinterpret_cast<const char*>(bytes.data()), bytes.size());
}

size_t ObjectId::getHashCode() const noexcept {
  return std::hash<folly::fbstring>{}(bytes_);
}

ObjectId ObjectId::sha1(const folly::IOBuf& buf) {
  Hash20::Storage hashBytes;
  OpenSSLHash::sha1(range(hashBytes), buf);
  return ObjectId{hashBytes};
}

ObjectId ObjectId::sha1(ByteRange data) {
  Hash20::Storage hashBytes;
  OpenSSLHash::sha1(range(hashBytes), data);
  return ObjectId{hashBytes};
}

void ObjectId::throwInvalidArgument(const char* message, size_t number) {
  throw_<std::invalid_argument>(message, number);
}

std::ostream& operator<<(std::ostream& os, const ObjectId& hash) {
  os << hash.toLogString();
  return os;
}

void toAppend(const ObjectId& hash, std::string* result) {
  folly::toAppend(hash.toLogString(), result);
}

} // namespace facebook::eden
