/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/model/ObjectId.h"

#include <folly/Conv.h>
#include <folly/String.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/ssl/OpenSSLHash.h>
#include <string>

#include "eden/common/utils/Throw.h"
#include "eden/fs/model/Hash.h"

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
  const char* p = bytes_.data();
  size_t n = bytes_.size();

  if (UNLIKELY(n < sizeof(uint64_t))) {
    size_t rv = 0;
    memcpy(&rv, p, n);
    return rv;
  }

  // unaligned load of tail
  size_t rv;
  size_t incrementSize = sizeof(uint64_t);
  memcpy(&rv, p + (n - incrementSize), incrementSize);
  for (const char* end = p + (n - incrementSize); p < end; p += incrementSize) {
    size_t x;
    memcpy(&x, p, incrementSize);
    rv ^= x;
  }
  return rv;
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

} // namespace facebook::eden
