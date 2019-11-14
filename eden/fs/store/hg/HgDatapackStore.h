/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Optional.h>
#include <folly/Range.h>
#include <folly/Synchronized.h>
#include <folly/io/IOBuf.h>

#include "eden/fs/model/Blob.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/scm/lib/backingstore/c_api/HgNativeBackingStore.h"

namespace facebook {
namespace eden {

class Hash;
class HgProxyHash;

class HgDatapackStore {
 public:
  explicit HgDatapackStore(AbsolutePathPiece repository)
      : store_{repository.stringPiece()} {}

  std::unique_ptr<Blob> getBlob(const Hash& id, const HgProxyHash& hgInfo);

  folly::Optional<folly::IOBuf> getTree(const Hash& id, RelativePath path);

 private:
  HgNativeBackingStore store_;
};
} // namespace eden
} // namespace facebook
