/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/Range.h>
#include <optional>
#include <string>

#include "eden/fs/model/Hash.h"
#include "eden/fs/service/EdenError.h"

namespace facebook {
namespace eden {

/**
 * Convert a Hash to a std::string to be returned via thrift as a thrift
 * BinaryHash data type.
 */
inline std::string thriftHash(const Hash& hash) {
  return folly::StringPiece{hash.getBytes()}.str();
}

/**
 * Convert an optional<Hash> to a std::string to be returned via thrift
 * as a thrift BinaryHash data type.
 */
inline std::string thriftHash(const std::optional<Hash>& hash) {
  if (hash.has_value()) {
    return thriftHash(hash.value());
  }
  return std::string{};
}

/**
 * TODO: remove this
 */
inline std::string thriftHash(const folly::Optional<Hash>& hash) {
  if (hash.has_value()) {
    return thriftHash(hash.value());
  }
  return std::string{};
}

/**
 * Convert thrift BinaryHash data type into a Hash object.
 *
 * This allows the input to be either a 20-byte binary string, or a 40-byte
 * hexadecimal string.
 */
inline Hash hashFromThrift(const std::string& commitID) {
  if (commitID.size() == Hash::RAW_SIZE) {
    // This looks like 20 bytes of binary data.
    return Hash(folly::ByteRange(folly::StringPiece(commitID)));
  } else if (commitID.size() == 2 * Hash::RAW_SIZE) {
    // This looks like 40 bytes of hexadecimal data.
    return Hash(commitID);
  } else {
    throw newEdenError(
        "expected argument to be a 20-byte binary hash or "
        "40-byte hexadecimal hash; got \"{}\"",
        commitID);
  }
}
} // namespace eden
} // namespace facebook
