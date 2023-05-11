/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/ThriftUtil.h"
#include <folly/String.h>

namespace facebook::eden {

RootId HashRootIdCodec::parseRootId(folly::StringPiece piece) {
  return RootId{hash20FromThrift(piece).toString()};
}

std::string HashRootIdCodec::renderRootId(const RootId& rootId) {
  return folly::unhexlify(rootId.value());
}

} // namespace facebook::eden
