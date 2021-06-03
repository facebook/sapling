/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <optional>
#include <string>

#include "eden/fs/model/Hash.h"
#include "eden/fs/model/RootId.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/utils/EdenError.h"

namespace facebook::eden {

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
 * Convert thrift BinaryHash data type into a Hash object.
 *
 * This allows the input to be either a 20-byte binary string, or a 40-byte
 * hexadecimal string.
 */
inline Hash hashFromThrift(folly::StringPiece commitID) {
  if (commitID.size() == Hash::RAW_SIZE) {
    // This looks like 20 bytes of binary data.
    return Hash(folly::ByteRange(folly::StringPiece(commitID)));
  } else if (commitID.size() == 2 * Hash::RAW_SIZE) {
    // This looks like 40 bytes of hexadecimal data.
    return Hash(commitID);
  } else {
    throw newEdenError(
        EINVAL,
        EdenErrorType::ARGUMENT_ERROR,
        "expected argument to be a 20-byte binary hash or "
        "40-byte hexadecimal hash; got \"",
        commitID,
        "\"");
  }
}

class HashRootIdCodec : public RootIdCodec {
 public:
  Hash parseRootId(folly::StringPiece piece) override {
    return hashFromThrift(piece);
  }
  std::string renderRootId(const Hash& rootId) override {
    return thriftHash(rootId);
  }
};

} // namespace facebook::eden
