/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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
#include "folly/String.h"

namespace facebook::eden {

/**
 * Convert a Hash to a std::string to be returned via thrift as a thrift
 * BinaryHash data type.
 */
inline std::string thriftHash(const ObjectId& hash) {
  return folly::StringPiece{hash.getBytes()}.str();
}

/**
 * Convert a Hash to a std::string to be returned via thrift as a thrift
 * BinaryHash data type.
 */
inline std::string thriftHash20(const Hash20& hash) {
  return folly::StringPiece{hash.getBytes()}.str();
}

/**
 * Convert thrift BinaryHash data type into a Hash20 object.
 *
 * This allows the input to be either a 20-byte binary string, or a 40-byte
 * hexadecimal string.
 */
inline Hash20 hash20FromThrift(folly::StringPiece commitID) {
  if (commitID.size() == Hash20::RAW_SIZE) {
    // This looks like 20 bytes of binary data.
    return Hash20(folly::ByteRange(folly::StringPiece(commitID)));
  } else if (commitID.size() == 2 * Hash20::RAW_SIZE) {
    // This looks like 40 bytes of hexadecimal data.
    return Hash20(commitID);
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

/**
 * A RootId codec suitable for BackingStores that use 20-byte hashes for
 * RootIds, like Git and Hg.
 */
class HashRootIdCodec : public RootIdCodec {
 public:
  RootId parseRootId(folly::StringPiece piece) override {
    return RootId{hash20FromThrift(piece).toString()};
  }
  std::string renderRootId(const RootId& rootId) override {
    return folly::unhexlify(rootId.value());
  }
};

} // namespace facebook::eden
