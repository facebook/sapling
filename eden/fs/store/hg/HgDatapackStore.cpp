/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgDatapackStore.h"

#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <memory>
#include <optional>

#include "eden/fs/model/Blob.h"
#include "eden/fs/store/hg/HgProxyHash.h"

namespace facebook {
namespace eden {

std::unique_ptr<Blob> HgDatapackStore::getBlob(
    const Hash& id,
    const HgProxyHash& hgInfo) {
  auto content =
      store_.getBlob(hgInfo.path().stringPiece(), hgInfo.revHash().getBytes());
  if (content) {
    return std::make_unique<Blob>(id, *content);
  }

  return nullptr;
}
} // namespace eden
} // namespace facebook
