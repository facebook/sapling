/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/model/Hash.h"

namespace facebook::eden {

// TODO: This will be expanded to a variable-width type soon.
using RootId = Hash;

/**
 * The meaning of a RootId is defined by the BackingStore implementation. Allow
 * it to also define how how root IDs are parsed and rendered at API boundaries
 * such as Thrift.
 */
class RootIdCodec {
 public:
  virtual ~RootIdCodec() = default;
  virtual RootId parseRootId(folly::StringPiece rootId) = 0;
  virtual std::string renderRootId(const RootId& rootId) = 0;
};

} // namespace facebook::eden
